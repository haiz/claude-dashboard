//! Claude Code's credential objects, carried verbatim (port of macOS
//! `Models/ClaudeCodeCredential.swift`). Only the fields the switcher decides on are parsed;
//! the canonical JSON keeps every field, so a write never drops one a newer Claude Code added.

use serde_json::Value;

/// Sorted-key, compact JSON of an object (nested objects sorted too); `None` for non-objects.
pub fn canonical(v: &Value) -> Option<String> {
    v.is_object().then(|| sorted(v).to_string())
}

fn sorted(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for k in keys {
                out.insert(k.clone(), sorted(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The `emailAddress` of an `oauthAccount` object given as JSON text.
pub fn email_of(account_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(account_json).ok()?;
    v.get("emailAddress")?.as_str().map(String::from)
}

fn millis(v: Option<&Value>) -> Option<i64> {
    v.and_then(Value::as_f64).map(|ms| ms as i64).filter(|&ms| ms > 0)
}

/// The `claudeAiOauth` object. Equal credentials have equal canonical JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthCredential {
    pub json: String,
    pub refresh_token: String,
    /// Epoch milliseconds; 0 or absent is `None`.
    pub expires_at_ms: Option<i64>,
    pub refresh_expires_at_ms: Option<i64>,
}

impl OAuthCredential {
    pub fn from_value(v: &Value) -> Option<Self> {
        Some(OAuthCredential {
            json: canonical(v)?,
            refresh_token: v.get("refreshToken").and_then(Value::as_str).unwrap_or_default().to_string(),
            expires_at_ms: millis(v.get("expiresAt")),
            refresh_expires_at_ms: millis(v.get("refreshTokenExpiresAt")),
        })
    }

    pub fn value(&self) -> Value {
        serde_json::from_str(&self.json).unwrap_or(Value::Null)
    }

    /// Claude Code blanks the credential after a failed refresh: empty tokens, `expiresAt` 0.
    pub fn is_blank(&self) -> bool {
        self.refresh_token.is_empty()
    }
}

/// What the app keeps per account: the credential plus the `oauthAccount` object
/// `.claude.json` must carry while it is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultEntry {
    pub oauth: OAuthCredential,
    /// Canonical JSON of the `oauthAccount` object.
    pub oauth_account: String,
}

impl VaultEntry {
    /// `{"claudeAiOauth": {...}, "oauthAccount": {...}}`.
    pub fn to_json(&self) -> String {
        let account: Value = serde_json::from_str(&self.oauth_account).unwrap_or(Value::Null);
        serde_json::json!({"claudeAiOauth": self.oauth.value(), "oauthAccount": account}).to_string()
    }

    pub fn from_json(text: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(text).ok()?;
        Some(VaultEntry {
            oauth: OAuthCredential::from_value(v.get("claudeAiOauth")?)?,
            oauth_account: canonical(v.get("oauthAccount")?)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn cred(refresh: &str, expires: i64, refresh_expires: i64) -> Value {
        json!({"accessToken": "at", "refreshToken": refresh, "expiresAt": expires,
               "refreshTokenExpiresAt": refresh_expires, "scopes": ["a"], "subscriptionType": "max"})
    }

    #[test]
    fn canonical_sorts_keys_and_rejects_non_objects() {
        let v = json!({"b": 1, "a": {"y": 2, "x": [3, {"q": 1, "p": 0}]}});
        assert_eq!(canonical(&v).as_deref(), Some(r#"{"a":{"x":[3,{"p":0,"q":1}],"y":2},"b":1}"#));
        assert_eq!(canonical(&json!([1])), None);
        assert_eq!(canonical(&json!("x")), None);
    }

    #[test]
    fn credential_parses_fields_and_blank() {
        let c = OAuthCredential::from_value(&cred("rt", 1000, 2000)).unwrap();
        assert_eq!(c.refresh_token, "rt");
        assert_eq!((c.expires_at_ms, c.refresh_expires_at_ms), (Some(1000), Some(2000)));
        assert!(!c.is_blank());
        assert_eq!(c.value()["scopes"], json!(["a"]), "every field kept");
        let blank = OAuthCredential::from_value(&cred("", 0, 2000)).unwrap();
        assert!(blank.is_blank());
        assert!(OAuthCredential::from_value(&json!(5)).is_none());
    }

    #[test]
    fn zero_expiry_is_none() {
        let c = OAuthCredential::from_value(&json!({"refreshToken": "r", "expiresAt": 0})).unwrap();
        assert_eq!((c.expires_at_ms, c.refresh_expires_at_ms), (None, None));
    }

    #[test]
    fn vault_entry_round_trips() {
        let e = VaultEntry {
            oauth: OAuthCredential::from_value(&cred("rt", 1, 2)).unwrap(),
            oauth_account: canonical(&json!({"emailAddress": "a@x.com", "accountUuid": "u"})).unwrap(),
        };
        let text = e.to_json();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(v.get("claudeAiOauth").is_some() && v.get("oauthAccount").is_some());
        assert_eq!(VaultEntry::from_json(&text), Some(e));
        assert_eq!(VaultEntry::from_json("{}"), None);
    }

    #[test]
    fn email_of_reads_email_address() {
        assert_eq!(email_of(r#"{"emailAddress":"a@x.com"}"#).as_deref(), Some("a@x.com"));
        assert_eq!(email_of("{}"), None);
        assert_eq!(email_of("nope"), None);
    }
}
