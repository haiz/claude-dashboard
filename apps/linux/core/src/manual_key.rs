//! What a pasted session key does to the store.
//!
//! Contract rule — `contract/cases/manual-key.json` drives this, and
//! `apps/macos/Shared/ManualKey.swift` must satisfy the same file.
//!
//! Dedupe (`contract/cases/dedupe.json`) decides *whether* a stored record
//! matches; this decides what happens next.

use crate::identity::{resolve_org_id, OrgMembership};

/// The fields of the stored record a pasted key matched, as this rule sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredManualTarget {
    pub org_id: Option<String>,
    pub account_uuid: Option<String>,
    pub email: Option<String>,
}

/// Fields the repair branch writes. `None` means **leave the stored value alone**.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ManualKeyWrites {
    pub org_id: Option<String>,
    pub account_uuid: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ManualKeyDecision {
    Add { org_id: String },
    RejectNoChatOrg,
    Repair { writes: ManualKeyWrites },
}

/// 1. No stored match: add the account, at the org `resolve_org_id` picks with
///    no `lastActiveOrg` preference (rule 2 of org selection). No chat org at
///    all rejects it — an unresolvable org is never persisted as working.
/// 2. A stored match: repair. `org_id` is **not** rewritten, because a pasted
///    key carries no org preference and re-resolving would demote an `org_id`
///    the cookie had resolved correctly. The exception is a stored `None`:
///    nothing to demote, same backfill semantics as `account_uuid`.
/// 3. A repair is never rejected for having no chat org. The key is still
///    saved, mirroring `resyncCore`, and the caller reports the org.
pub fn manual_key_decision(
    stored: Option<&StoredManualTarget>,
    fetched_uuid: &str,
    fetched_email: Option<&str>,
    memberships: &[OrgMembership],
) -> ManualKeyDecision {
    let resolved = resolve_org_id(None, memberships);

    let Some(stored) = stored else {
        return match resolved {
            Some(org_id) => ManualKeyDecision::Add { org_id },
            None => ManualKeyDecision::RejectNoChatOrg,
        };
    };

    ManualKeyDecision::Repair {
        writes: ManualKeyWrites {
            org_id: if stored.org_id.is_none() { resolved } else { None },
            account_uuid: if stored.account_uuid.is_none() {
                Some(fetched_uuid.to_string())
            } else {
                None
            },
            email: if stored.email.is_none() {
                fetched_email.map(String::from)
            } else {
                None
            },
        },
    }
}

/// The key with surrounding whitespace and newlines removed, or `None` when
/// nothing is left. Mirrors `ManualKeyInput.trimmedKey` in Swift.
///
/// Deliberately no format check: guessing at a prefix would break the day the
/// format changes, and `/api/account` is the real validator.
pub fn trimmed_key(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_key_strips_the_edges_only() {
        assert_eq!(trimmed_key("sk-abc\n"), Some("sk-abc"));
        assert_eq!(trimmed_key("  sk-abc  "), Some("sk-abc"));
        assert_eq!(trimmed_key("sk-a.b-c_d\n"), Some("sk-a.b-c_d"));
    }

    #[test]
    fn trimmed_key_is_none_for_whitespace() {
        assert_eq!(trimmed_key(" \n\t "), None);
        assert_eq!(trimmed_key(""), None);
    }
}
