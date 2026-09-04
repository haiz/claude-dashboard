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
        self.capabilities.iter().any(|c| c.to_lowercase() == "chat")
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

/// The identity fields of an already-stored account, as dedupe sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredIdentity {
    pub account_uuid: Option<String>,
    pub email: Option<String>,
}

/// Which stored record `candidate_uuid` names, if any — the position of the
/// first match, in store order.
///
/// 1. a stored `account_uuid` equal to the candidate's — duplicate
/// 2. a stored record with **no** `account_uuid` whose email matches
///    after Unicode lowercasing (`to_lowercase` both sides, then compare)
///    — duplicate. Legacy records only.
/// 3. otherwise not a duplicate
///
/// `org_id` is never consulted.
///
/// [`is_duplicate`] is this predicate. The index exists because `sync`'s plan
/// heal needs the record it matched, and re-deriving that match at the call
/// site is exactly the drift `contract/cases/dedupe.json` exists to prevent.
pub fn duplicate_index(
    candidate_uuid: &str,
    candidate_email: Option<&str>,
    stored: &[StoredIdentity],
) -> Option<usize> {
    stored.iter().position(|entry| match (&entry.account_uuid, &entry.email, candidate_email) {
        (Some(uuid), _, _) => uuid == candidate_uuid,
        (None, Some(stored_email), Some(cand)) => stored_email.to_lowercase() == cand.to_lowercase(),
        _ => false,
    })
}

/// Whether `candidate_uuid` names an account already in the store. See
/// [`duplicate_index`] for the rule.
pub fn is_duplicate(
    candidate_uuid: &str,
    candidate_email: Option<&str>,
    stored: &[StoredIdentity],
) -> bool {
    duplicate_index(candidate_uuid, candidate_email, stored).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(uuid: Option<&str>, email: Option<&str>) -> StoredIdentity {
        StoredIdentity {
            account_uuid: uuid.map(String::from),
            email: email.map(String::from),
        }
    }

    #[test]
    fn duplicate_index_points_at_the_matching_entry() {
        let store = vec![
            stored(Some("acct-1"), Some("one@example.com")),
            stored(Some("acct-2"), Some("two@example.com")),
        ];
        assert_eq!(duplicate_index("acct-2", Some("two@example.com"), &store), Some(1));
    }

    #[test]
    fn duplicate_index_finds_the_legacy_email_match() {
        let store = vec![
            stored(Some("acct-1"), Some("one@example.com")),
            stored(None, Some("Legacy@Example.com")),
        ];
        assert_eq!(duplicate_index("acct-9", Some("legacy@example.com"), &store), Some(1));
    }

    #[test]
    fn duplicate_index_is_none_for_a_new_account() {
        let store = vec![stored(Some("acct-1"), Some("one@example.com"))];
        assert_eq!(duplicate_index("acct-2", Some("two@example.com"), &store), None);
    }
}
