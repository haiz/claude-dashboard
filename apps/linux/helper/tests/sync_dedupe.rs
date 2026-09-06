//! The `sync` subcommand's dedupe key. Pins the helper to the shared rule in
//! `claude-dashboard-core` — two literal cases against `is_duplicate`. The
//! `contract/cases/dedupe.json` cases are driven by
//! `apps/linux/core/tests/contract_dedupe.rs`, not here.

use claude_dashboard_core::identity::{is_duplicate, StoredIdentity};

#[test]
fn two_members_of_one_org_are_not_duplicates() {
    let stored = vec![StoredIdentity {
        account_uuid: Some("acct-1".into()),
        email: Some("person@example.com".into()),
    }];
    assert!(!is_duplicate("acct-2", Some("other@example.com"), &stored));
}

#[test]
fn the_same_account_seen_again_is_a_duplicate() {
    let stored = vec![StoredIdentity {
        account_uuid: Some("acct-1".into()),
        email: Some("person@example.com".into()),
    }];
    assert!(is_duplicate("acct-1", Some("person@example.com"), &stored));
}
