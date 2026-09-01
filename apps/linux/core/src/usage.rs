//! Decodes the claude.ai `/api/organizations/{orgId}/usage` response.
//!
//! Mirrors `apps/macos/Shared/UsageData.swift`'s custom `init(from:)`; see
//! `contract/README.md`'s "The Fable window" and "Timestamps" sections for
//! the rules this module must reproduce exactly, and
//! `contract/cases/usage-decoding.json` for the pinned cases (read directly
//! by `tests/contract_usage.rs`, not copied here).

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct UsageLimit {
    pub utilization: f64,
    /// Unix seconds, truncated toward zero. Absent or JSON `null` in the
    /// source is preserved as `None`, not defaulted.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageData {
    pub five_hour: UsageLimit,
    pub seven_day: UsageLimit,
    /// Derived from `limits[]`, not a top-level field. `None` when no entry
    /// has `scope.model.display_name == "Fable"`, when `limits` is absent,
    /// or when `limits` is malformed (see `derive_fable`).
    pub fable: Option<UsageLimit>,
}

#[derive(Debug, PartialEq)]
pub enum UsageError {
    /// The response body is not valid JSON.
    NotObject,
    /// A required window (`five_hour` / `seven_day`) key is missing.
    MissingWindow(&'static str),
    /// A required window's `utilization` is missing or not a number.
    BadUtilization(&'static str),
    /// A required window's `resets_at` is present but not a parseable
    /// RFC3339 timestamp (absent/null is fine — see `UsageLimit::resets_at`).
    BadResetsAt(&'static str),
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageError::NotObject => write!(f, "usage response is not valid JSON"),
            UsageError::MissingWindow(k) => write!(f, "missing required window `{k}`"),
            UsageError::BadUtilization(k) => write!(f, "`{k}.utilization` missing or not a number"),
            UsageError::BadResetsAt(k) => write!(f, "`{k}.resets_at` present but unparseable"),
        }
    }
}

impl std::error::Error for UsageError {}

impl UsageData {
    pub fn decode(json: &str) -> Result<UsageData, UsageError> {
        let v: Value = serde_json::from_str(json).map_err(|_| UsageError::NotObject)?;
        let five_hour = required_window(&v, "five_hour")?;
        let seven_day = required_window(&v, "seven_day")?;
        let fable = derive_fable(&v); // swallows all errors -> None
        Ok(UsageData { five_hour, seven_day, fable })
    }
}

fn required_window(root: &Value, key: &'static str) -> Result<UsageLimit, UsageError> {
    let obj = root.get(key).ok_or(UsageError::MissingWindow(key))?;
    let utilization = obj
        .get("utilization")
        .and_then(Value::as_f64)
        .ok_or(UsageError::BadUtilization(key))?;
    let resets_at = parse_resets_at_required(obj.get("resets_at"), key)?;
    Ok(UsageLimit { utilization, resets_at })
}

/// Absent or null -> `Ok(None)`. Present string that won't parse -> `Err`.
/// Present non-string -> `Err`.
fn parse_resets_at_required(v: Option<&Value>, key: &'static str) -> Result<Option<i64>, UsageError> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            parse_iso8601_to_unix_trunc(s).map(Some).ok_or(UsageError::BadResetsAt(key))
        }
        Some(_) => Err(UsageError::BadResetsAt(key)),
    }
}

/// One `limits[]` entry, fully validated and parsed.
struct ParsedLimitEntry {
    is_fable: bool,
    utilization: f64,
    resets_at: Option<i64>,
}

/// Derives the Fable window from `limits[]`, swallowing any malformation
/// into `None` rather than failing the outer decode — this mirrors Swift's
/// `(try? container.decode([LimitEntry].self, forKey: .limits)) ?? []`
/// (`contract/README.md` "The Fable window").
///
/// Swift decodes the *entire* array into `[LimitEntry]` before running
/// `.first { ... }` on it, so a malformation on ANY entry — not only the one
/// that would end up matching Fable — poisons the whole array and the
/// `try?` swallows it to `[]`. This must be a validate-then-select
/// two-phase walk, not a single pass that could return early on the first
/// Fable match while a later entry is still unvalidated.
fn derive_fable(root: &Value) -> Option<UsageLimit> {
    let limits = root.get("limits")?;
    let arr = limits.as_array()?; // non-array -> None (swallow)

    let mut parsed = Vec::with_capacity(arr.len());
    for entry in arr {
        let obj = entry.as_object()?; // non-object entry -> None (swallow, matches try? on whole array)

        let utilization = match obj.get("percent") {
            None | Some(Value::Null) => 0.0,
            Some(v) => v.as_f64()?, // non-numeric percent on ANY entry -> None (swallow)
        };
        let resets_at = match obj.get("resets_at") {
            None | Some(Value::Null) => None,
            // unparseable resets_at on ANY entry -> None (swallow)
            Some(Value::String(s)) => Some(parse_iso8601_to_unix_trunc(s)?),
            Some(_) => return None,
        };
        let is_fable = obj
            .get("scope")
            .and_then(|s| s.get("model"))
            .and_then(|m| m.get("display_name"))
            .and_then(Value::as_str)
            == Some("Fable");
        parsed.push(ParsedLimitEntry { is_fable, utilization, resets_at });
    }

    parsed
        .into_iter()
        .find(|p| p.is_fable)
        .map(|p| UsageLimit { utilization: p.utilization, resets_at: p.resets_at })
}

/// Parses both `...:59.661633+00:00` (fractional) and `...:59+00:00`
/// (whole-second) RFC3339 forms, truncating toward zero to whole seconds.
/// This is the ISO8601 quirk the macOS decoder handles with two
/// `DateFormatter`s (`contract/README.md` "Timestamps").
pub fn parse_iso8601_to_unix_trunc(s: &str) -> Option<i64> {
    let odt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()?;
    // `unix_timestamp()` drops the sub-second component entirely (it is not
    // derived from a rounded f64), so this truncates toward zero for the
    // post-1970 timestamps the API emits.
    Some(odt.unix_timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal(extra: &str) -> String {
        format!(
            r#"{{"five_hour":{{"utilization":1.0,"resets_at":null}},
                 "seven_day":{{"utilization":2.0,"resets_at":null}}{extra}}}"#
        )
    }

    #[test]
    fn truncates_fractional_seconds_toward_zero() {
        // .661633 must be dropped, not rounded up to the next second.
        assert_eq!(
            parse_iso8601_to_unix_trunc("2026-04-10T18:59:59.661633+00:00"),
            Some(1_775_847_599)
        );
    }

    #[test]
    fn missing_five_hour_fails_whole_decode() {
        let json = r#"{"seven_day":{"utilization":2.0,"resets_at":null}}"#;
        assert_eq!(UsageData::decode(json), Err(UsageError::MissingWindow("five_hour")));
    }

    #[test]
    fn non_numeric_utilization_fails_whole_decode() {
        let json = r#"{"five_hour":{"utilization":"bad","resets_at":null},
                        "seven_day":{"utilization":2.0,"resets_at":null}}"#;
        assert_eq!(UsageData::decode(json), Err(UsageError::BadUtilization("five_hour")));
    }

    #[test]
    fn unparseable_resets_at_on_required_window_fails_whole_decode() {
        let json = r#"{"five_hour":{"utilization":1.0,"resets_at":"not-a-date"},
                        "seven_day":{"utilization":2.0,"resets_at":null}}"#;
        assert_eq!(UsageData::decode(json), Err(UsageError::BadResetsAt("five_hour")));
    }

    #[test]
    fn limits_not_an_array_is_swallowed_to_no_fable() {
        let json = minimal(r#","limits":"not-an-array""#);
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, None);
    }

    #[test]
    fn limits_entry_not_an_object_is_swallowed_to_no_fable() {
        let json = minimal(r#","limits":[1,2,3]"#);
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, None);
    }

    #[test]
    fn non_numeric_percent_on_fable_entry_is_swallowed_to_no_fable() {
        let json = minimal(
            r#","limits":[{"percent":"bad","resets_at":null,
                "scope":{"model":{"display_name":"Fable"}}}]"#,
        );
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, None);
    }

    #[test]
    fn unparseable_resets_at_on_non_fable_entry_before_fable_swallows_whole_limits() {
        // A bad resets_at on an unrelated entry still poisons the whole
        // array decode in Swift, so it must swallow to None here too, even
        // though a real Fable entry follows it.
        let json = minimal(
            r#","limits":[
                {"percent":9,"resets_at":"nope","scope":null},
                {"percent":100,"resets_at":null,"scope":{"model":{"display_name":"Fable"}}}
            ]"#,
        );
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, None);
    }

    #[test]
    fn unparseable_resets_at_on_non_fable_entry_after_fable_swallows_whole_limits() {
        // Regression: the Fable entry comes FIRST here. A validate-as-you-go
        // walk that returns Some the moment it finds Fable would miss the
        // later malformed entry and wrongly report a Fable window — Swift
        // decodes the whole array before selecting, so this must swallow.
        let json = minimal(
            r#","limits":[
                {"percent":100,"resets_at":null,"scope":{"model":{"display_name":"Fable"}}},
                {"percent":9,"resets_at":"nope","scope":null}
            ]"#,
        );
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, None);
    }

    #[test]
    fn non_object_entry_after_fable_swallows_whole_limits() {
        let json = minimal(
            r#","limits":[
                {"percent":100,"resets_at":null,"scope":{"model":{"display_name":"Fable"}}},
                42
            ]"#,
        );
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, None);
    }

    #[test]
    fn non_numeric_percent_on_non_fable_entry_swallows_whole_limits() {
        // Also covers "before Fable": the bad entry sorts first and would
        // never reach the old Fable-only percent check at all.
        let json = minimal(
            r#","limits":[
                {"percent":"bad","resets_at":null,"scope":null},
                {"percent":100,"resets_at":null,"scope":{"model":{"display_name":"Fable"}}}
            ]"#,
        );
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, None);
    }

    #[test]
    fn missing_percent_on_fable_entry_defaults_to_zero() {
        let json = minimal(
            r#","limits":[{"resets_at":null,"scope":{"model":{"display_name":"Fable"}}}]"#,
        );
        let d = UsageData::decode(&json).unwrap();
        assert_eq!(d.fable, Some(UsageLimit { utilization: 0.0, resets_at: None }));
    }
}
