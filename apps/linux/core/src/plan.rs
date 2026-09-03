use crate::model::AccountPlan;
use serde_json::Value;

/// Derives the account's plan tier from one `GET /api/organizations` entry.
///
/// Mirrors `UsageAPIService.detectPlanTier` (`apps/macos/Shared/UsageAPIService.swift:101-123`)
/// and `contract/README.md`'s "Plan tier" section, in this exact order:
///
/// 1. The org's raw JSON, serialized back to a lowercased string, contains
///    `"max_20x"` or `"max20x"` -> `Max20x`.
/// 2. Else contains `"max_5x"` or `"max5x"` -> `Max5x`.
/// 3. Else `capabilities` contains `"claude_pro"` -> `Pro` (checked before the
///    chat fallback, since Pro orgs also carry `"chat"`).
/// 4. Else `capabilities` contains `"claude_max"` -> `Max200` (wire `"Max"`).
/// 5. Else `capabilities` contains `"chat"` -> `Max200` (wire `"Max"`) — a
///    consumer chat org without the Pro marker, tier unknown.
/// 6. Else `None` — not a plan the dashboard displays (e.g. an API-only org).
///
/// Steps 1-2 scan the *entire* serialized org, not just `capabilities` — a
/// marker anywhere in the org JSON (e.g. in `name`) triggers it. Capability
/// matching in steps 3-5 is case-insensitive, matching the Swift source's
/// `Set(capabilities.map { $0.lowercased() })`.
pub fn detect_plan_tier(org: &Value, capabilities: &[String]) -> Option<AccountPlan> {
    let raw = serde_json::to_string(org).unwrap_or_default().to_lowercase();
    if raw.contains("max_20x") || raw.contains("max20x") {
        return Some(AccountPlan::Max20x);
    }
    if raw.contains("max_5x") || raw.contains("max5x") {
        return Some(AccountPlan::Max5x);
    }

    let has = |c: &str| capabilities.iter().any(|x| x.to_lowercase() == c);
    if has("claude_pro") {
        return Some(AccountPlan::Pro);
    }
    if has("claude_max") {
        return Some(AccountPlan::Max200);
    }
    if has("chat") {
        return Some(AccountPlan::Max200);
    }
    None
}

/// The plan to persist for an already-stored account, given a freshly fetched
/// `hint` — `None` means *leave the stored plan alone*.
///
/// Mirrors `UsageAPIService.refreshedPlan` and `contract/README.md`'s
/// "Refreshing a stored plan"; `contract/cases/plan-refresh.json` is the rule:
///
/// 1. `hint` is `None` (fetch failed, no org matched the account's `orgId`, or
///    [`detect_plan_tier`] found no displayable plan) -> `None`. A blip must
///    never overwrite a known-good tier, and `sync`'s add-time `Pro` default is
///    never re-fabricated here.
/// 2. `hint` equals `stored` -> `None`, so callers can treat a `Some` as "this
///    is a real change worth writing and reporting".
/// 3. Otherwise -> `Some(hint)`, in either direction: an upgrade and a
///    downgrade are the same case, the fetched org is authoritative.
pub fn refreshed_plan(stored: &AccountPlan, hint: Option<AccountPlan>) -> Option<AccountPlan> {
    match hint {
        Some(hint) if hint != *stored => Some(hint),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capability_matching_is_case_insensitive() {
        let org = json!({"uuid": "org-9", "name": "Personal"});
        let caps = vec!["CHAT".to_string(), "Claude_Pro".to_string()];
        assert_eq!(detect_plan_tier(&org, &caps), Some(AccountPlan::Pro));
    }

    #[test]
    fn no_markers_and_no_capabilities_is_none() {
        let org = json!({"uuid": "org-5", "name": "Empty"});
        assert_eq!(detect_plan_tier(&org, &[]), None);
    }
}
