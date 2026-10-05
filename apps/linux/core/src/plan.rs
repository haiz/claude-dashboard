use crate::model::{Account, AccountPlan};
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

/// The plan to persist for `account` given a freshly fetched
/// `/api/organizations` result — `None` to leave the stored plan alone.
///
/// Mirrors `UsageAPIService.refreshedPlan(for:orgs:)`. The org is matched on
/// the account's **stored** `org_id`: an account with no `org_id` is not
/// pollable and is never touched, and an `orgs` slice with no matching entry
/// (an empty one included, which is what a failed fetch produces) reduces to
/// rule 1 of [`refreshed_plan`].
///
/// Deliberately no `unwrap_or(Pro)` here: unlike the add path
/// ([`plan_for`]), an unresolved tier must leave the stored one as it is.
pub fn refreshed_plan_for(account: &Account, orgs: &[ParsedOrg]) -> Option<AccountPlan> {
    let org_id = account.org_id.as_deref()?;
    let hint = orgs
        .iter()
        .find(|o| o.uuid == org_id)
        .and_then(|o| detect_plan_tier(&o.raw, &o.capabilities));
    refreshed_plan(&account.plan, hint)
}

/// One parsed `/api/organizations` entry (only orgs carrying both `uuid`
/// and `name` survive, matching the Swift `compactMap`). Read solely for the
/// plan tier — e-mail comes from `/api/account`.
pub struct ParsedOrg {
    pub uuid: String,
    pub capabilities: Vec<String>,
    /// The org's full JSON, handed to [`detect_plan_tier`] (steps 1-2 there
    /// scan the whole object, not just `capabilities`).
    pub raw: Value,
}

/// Parses the raw `/api/organizations` body into the orgs the dashboard cares
/// about. Returns an empty vec when the body is not a JSON array, is empty,
/// or contains no org with both `uuid` and `name`.
pub fn parse_orgs(orgs_json: &str) -> Vec<ParsedOrg> {
    let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(orgs_json) else {
        return Vec::new();
    };
    arr.into_iter()
        .filter_map(|v| {
            let uuid = v.get("uuid")?.as_str()?.to_string();
            // Presence check only: a name-less org is filtered out, matching
            // the Swift `compactMap`. The name itself is never inspected.
            v.get("name")?.as_str()?;
            let capabilities = v
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(ParsedOrg {
                uuid,
                capabilities,
                raw: v,
            })
        })
        .collect()
}

/// Plan tier for the chosen org: `detect_plan_tier` on that org's raw JSON,
/// defaulting to Pro when the org is absent or yields nothing. The add path's
/// counterpart to [`refreshed_plan_for`] (which leaves an unresolved tier
/// alone instead of defaulting).
pub fn plan_for(orgs: &[ParsedOrg], org_id: &str) -> AccountPlan {
    orgs.iter()
        .find(|o| o.uuid == org_id)
        .and_then(|o| detect_plan_tier(&o.raw, &o.capabilities))
        .unwrap_or(AccountPlan::Pro)
}

/// The plan's on-the-wire string (`"Pro"`, `"Max 5x"`, `"Max 20x"`,
/// `"Max"`) — what the Swift `plan.rawValue` prints in the "Added:" line.
pub fn plan_wire_value(plan: &AccountPlan) -> String {
    match serde_json::to_value(plan) {
        Ok(Value::String(s)) => s,
        _ => String::new(),
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
