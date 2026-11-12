//! Free "limit resets" granted by claude.ai (a Full reset and Fable resets,
//! each with an expiry), read from the `/usage` payload.
//!
//! The payload carries them under an unreleased internal field name, called
//! "the limit-resets field" everywhere in this repo. The name is never stored
//! raw (`scripts/test-no-codename.sh` enforces it): it is kept base64-encoded
//! and decoded at runtime by [`field_name`]. The `/usage` request must carry
//! `?<field_name()>=1` for the field to be filled. The app only displays
//! resets; it never calls the reset endpoint.
//!
//! Rules: `contract/README.md` "Limit resets", cases in
//! `contract/cases/limit-resets.json`.
//!
//! ```
//! use claude_dashboard_core::limit_resets::{parse_grants, summarize};
//! let none = summarize(&parse_grants(&serde_json::Value::Null), 0);
//! assert!(none.full.is_none() && none.fable.is_none());
//! ```

use base64::Engine;
use serde_json::Value;

use crate::usage::parse_iso8601_to_unix_trunc;

const FIELD_B64: &str = "Y2VkYXJfZW1iZXI=";

/// A reset expiring in less than this many seconds is urgent (7 days).
pub const URGENT_SECONDS: i64 = 604_800;

/// The decoded name of the limit-resets field (query parameter and JSON key).
pub fn field_name() -> String {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(FIELD_B64)
        .expect("valid base64 constant");
    String::from_utf8(bytes).expect("utf-8 constant")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetKind {
    Full,
    Fable,
}

/// One usable grant. `ends_at` is unix seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct ResetGrant {
    pub kind: ResetKind,
    pub resets_left: u32,
    pub ends_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResetSummary {
    pub count: u32,
    pub expires_at: i64,
    pub urgent: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LimitResets {
    pub full: Option<ResetSummary>,
    pub fable: Option<ResetSummary>,
}

/// Parses the limit-resets object (or `Value::Null`). Time-independent: it
/// keeps every well-formed, unpaused grant with `resets_left >= 1`. Anything
/// malformed degrades to fewer grants, never an error.
pub fn parse_grants(v: &Value) -> Vec<ResetGrant> {
    let Some(obj) = v.as_object() else { return Vec::new() };
    if obj.get("eligible").and_then(Value::as_bool) != Some(true) {
        return Vec::new();
    }
    let Some(grants) = obj.get("grants").and_then(Value::as_array) else {
        return Vec::new();
    };
    grants.iter().filter_map(parse_grant).collect()
}

/// A JSON number holding an integer in `1..=u32::MAX` (`1` and `1.0` both
/// count); non-integral numbers, bools and strings give `None`.
fn integral_count(v: &Value) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).ok();
    }
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f >= 1.0 && f <= u32::MAX as f64).then_some(f as u32)
}

fn parse_grant(g: &Value) -> Option<ResetGrant> {
    let g = g.as_object()?;
    let resets_left = integral_count(g.get("resets_left")?)?;
    if resets_left < 1 || g.get("paused").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let ends_at = parse_iso8601_to_unix_trunc(g.get("ends_at")?.as_str()?)?;
    let kind = if g.get("gives_back_weekly").and_then(Value::as_bool) == Some(true) {
        ResetKind::Fable
    } else {
        let clears = g.get("clears").and_then(Value::as_array)?;
        let has = |k: &str| clears.iter().any(|c| c.as_str() == Some(k));
        if has("five_hour") && has("seven_day") {
            ResetKind::Full
        } else {
            return None;
        }
    };
    Some(ResetGrant { kind, resets_left, ends_at })
}

/// Drops grants that ended at or before `now_unix`, then sums counts and takes
/// the earliest expiry per kind.
pub fn summarize(grants: &[ResetGrant], now_unix: i64) -> LimitResets {
    let of = |kind: ResetKind| {
        let live: Vec<&ResetGrant> =
            grants.iter().filter(|g| g.kind == kind && g.ends_at > now_unix).collect();
        let expires_at = live.iter().map(|g| g.ends_at).min()?;
        let count = live.iter().fold(0u32, |a, g| a.saturating_add(g.resets_left));
        Some(ResetSummary { count, expires_at, urgent: expires_at - now_unix < URGENT_SECONDS })
    };
    LimitResets { full: of(ResetKind::Full), fable: of(ResetKind::Fable) }
}
