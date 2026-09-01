//! Which account this is, and which org to query usage from.
//!
//! Contract rules — `contract/cases/org-selection.json` and
//! `contract/cases/dedupe.json` drive these, and
//! `apps/macos/Shared/AccountIdentity.swift` must satisfy the same two files.

/// One organisation the account belongs to, from `GET /api/account`'s
/// `memberships[].organization`.
#[derive(Debug, Clone, PartialEq)]
pub struct OrgMembership {
    pub uuid: String,
    pub name: String,
    pub capabilities: Vec<String>,
}

impl OrgMembership {
    pub fn is_chat_org(&self) -> bool {
        self.capabilities.iter().any(|c| c.eq_ignore_ascii_case("chat"))
    }
}

/// The org whose `/usage` the account should be polled against.
///
/// 1. the cookie's `lastActiveOrg`, if that uuid is a chat org in `memberships`
/// 2. otherwise the first chat org, in the order the API returned them
/// 3. otherwise `None` — the account is not configurable
///
/// The org *name* is never inspected.
pub fn resolve_org_id(
    last_active_org: Option<&str>,
    memberships: &[OrgMembership],
) -> Option<String> {
    if let Some(want) = last_active_org {
        if let Some(org) = memberships.iter().find(|m| m.uuid == want) {
            if org.is_chat_org() {
                return Some(org.uuid.clone());
            }
        }
    }
    memberships.iter().find(|m| m.is_chat_org()).map(|m| m.uuid.clone())
}
