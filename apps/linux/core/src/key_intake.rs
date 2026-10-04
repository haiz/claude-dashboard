//! What a session key does to the account list: add a new account or repair
//! the stored one. Shared by the Linux helper's `add-key` and the Windows
//! native-messaging bridge, so both follow `contract/cases/dedupe.json` and
//! `contract/cases/manual-key.json` through one implementation.
//!
//! No I/O: the caller fetches `/api/account` and `/api/organizations`, holds
//! the store lock, loads the accounts, calls [`apply_session_key`], and saves.

use crate::api::ParsedAccount;
use crate::identity::{duplicate_index, StoredIdentity};
use crate::manual_key::{manual_key_decision, ManualKeyDecision, StoredManualTarget};
use crate::model::{Account, AccountPlan, AccountSource, AccountStatus, Browser};
use crate::plan::{plan_for, refreshed_plan_for, ParsedOrg};
use crate::store::encrypt_session_key;

/// The outcome of applying a key, for the caller to report. Mirrors the three
/// branches of `apply_session_key`.
#[derive(Debug, Clone, PartialEq)]
pub enum IntakeOutcome {
    Added {
        account_id: String,
        name: String,
        plan: AccountPlan,
    },
    /// `name` is the stored email, else the stored name. `warn_no_chat_org` is
    /// passed through from `manual_key_decision` — the resolve result, not a
    /// property of the stored record.
    Updated {
        account_id: String,
        name: String,
        old_plan: AccountPlan,
        new_plan: AccountPlan,
        warn_no_chat_org: bool,
    },
    RejectNoChatOrg,
}

/// Applies `session_key` (already validated by `/api/account` into
/// `identity`) to `accounts`. `orgs` is the parsed `/api/organizations` body,
/// empty when that fetch failed. `new_id` mints the id of an added account;
/// `now_reference` is "now" in Foundation reference-date seconds.
///
/// Mutates `accounts` in place; never loads, saves or fetches.
pub fn apply_session_key(
    accounts: &mut Vec<Account>,
    session_key: &str,
    identity: &ParsedAccount,
    orgs: &[ParsedOrg],
    new_id: impl FnOnce() -> String,
    now_reference: f64,
) -> IntakeOutcome {
    let stored: Vec<StoredIdentity> = accounts
        .iter()
        .map(|a| StoredIdentity {
            account_uuid: a.account_uuid.clone(),
            email: a.email.clone(),
        })
        .collect();
    let index = duplicate_index(&identity.uuid, identity.email.as_deref(), &stored);

    let target = index.map(|i| StoredManualTarget {
        org_id: accounts[i].org_id.clone(),
        account_uuid: accounts[i].account_uuid.clone(),
        email: accounts[i].email.clone(),
    });

    match manual_key_decision(
        target.as_ref(),
        &identity.uuid,
        identity.email.as_deref(),
        &identity.memberships,
    ) {
        ManualKeyDecision::RejectNoChatOrg => IntakeOutcome::RejectNoChatOrg,

        ManualKeyDecision::Add { org_id } => {
            let name = identity.email.clone().unwrap_or_else(|| {
                // Chars, not bytes: a multi-byte character crossing byte 8 would
                // panic a byte slice, and Swift's `.prefix(8)` does not.
                format!("Account {}", identity.uuid.chars().take(8).collect::<String>())
            });
            let plan = plan_for(orgs, &org_id);
            let account_id = new_id();
            accounts.push(Account {
                id: account_id.clone(),
                name: name.clone(),
                email: identity.email.clone(),
                chrome_profile_path: String::new(),
                chrome_profile_name: None,
                org_id: Some(org_id),
                account_uuid: Some(identity.uuid.clone()),
                session_key: Some(encrypt_session_key(session_key)),
                browser: Browser::Chrome,
                plan: plan.clone(),
                last_synced: Some(now_reference),
                status: AccountStatus::Active,
                is_pinned: false,
                source: AccountSource::Manual,
            });
            IntakeOutcome::Added {
                account_id,
                name,
                plan,
            }
        }

        ManualKeyDecision::Repair {
            writes,
            warn_no_chat_org,
        } => {
            let i = index.expect("manual_key_decision repairs only a stored match");
            let old_plan = accounts[i].plan.clone();
            accounts[i].session_key = Some(encrypt_session_key(session_key));
            accounts[i].status = AccountStatus::Active;
            accounts[i].last_synced = Some(now_reference);
            if let Some(org_id) = writes.org_id {
                accounts[i].org_id = Some(org_id);
            }
            if let Some(uuid) = writes.account_uuid {
                accounts[i].account_uuid = Some(uuid);
            }
            if let Some(email) = writes.email {
                accounts[i].email = Some(email);
            }
            // `refreshed_plan_for` matches the org against `account.org_id` as
            // it stands, so a `None` just filled in above is what gets matched.
            if let Some(plan) = refreshed_plan_for(&accounts[i], orgs) {
                accounts[i].plan = plan;
            }
            IntakeOutcome::Updated {
                account_id: accounts[i].id.clone(),
                name: accounts[i].email.clone().unwrap_or_else(|| accounts[i].name.clone()),
                old_plan,
                new_plan: accounts[i].plan.clone(),
                warn_no_chat_org,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::parse_account;
    use crate::plan::parse_orgs;
    use crate::store::decrypt_session_key;

    fn identity(uuid: &str, email: Option<&str>, caps: &str) -> ParsedAccount {
        let email_field = email.map(|e| format!(r#""email_address":"{e}","#)).unwrap_or_default();
        parse_account(&format!(
            r#"{{{email_field}"uuid":"{uuid}","memberships":[
                {{"organization":{{"uuid":"org-1","name":"Org","capabilities":{caps}}}}}]}}"#
        ))
        .unwrap()
    }

    fn orgs(caps: &str) -> Vec<ParsedOrg> {
        parse_orgs(&format!(r#"[{{"uuid":"org-1","name":"Org","capabilities":{caps}}}]"#))
    }

    fn stored(uuid: Option<&str>, email: &str) -> Account {
        let uuid_field = uuid.map(|u| format!(r#""accountUuid":"{u}","#)).unwrap_or_default();
        Account::from_json_object(&format!(
            r#"{{"id":"OLD-ID","name":"{email}","email":"{email}",{uuid_field}
                "chromeProfilePath":"","orgId":"org-1","plan":"Pro","status":"expired",
                "source":"manual"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_new_identity_is_added_as_a_manual_active_account() {
        let mut accounts = Vec::new();
        let outcome = apply_session_key(
            &mut accounts,
            "sk-new",
            &identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#),
            &orgs(r#"["chat","claude_max"]"#),
            || "NEW-ID".to_string(),
            42.0,
        );
        assert_eq!(
            outcome,
            IntakeOutcome::Added {
                account_id: "NEW-ID".into(),
                name: "a@x.com".into(),
                plan: AccountPlan::Max200,
            }
        );
        let a = &accounts[0];
        assert_eq!(a.account_uuid.as_deref(), Some("acct-1"));
        assert_eq!(a.org_id.as_deref(), Some("org-1"));
        assert_eq!(a.source, AccountSource::Manual);
        assert_eq!(a.status, AccountStatus::Active);
        assert_eq!(a.last_synced, Some(42.0));
        assert_eq!(
            decrypt_session_key(a.session_key.as_deref().unwrap()).as_deref(),
            Some("sk-new")
        );
    }

    #[test]
    fn an_identity_without_email_is_named_from_its_uuid() {
        let mut accounts = Vec::new();
        let outcome = apply_session_key(
            &mut accounts,
            "sk",
            &identity("abcdefghijk", None, r#"["chat"]"#),
            &[],
            || "ID".to_string(),
            0.0,
        );
        assert!(matches!(outcome, IntakeOutcome::Added { ref name, .. } if name == "Account abcdefgh"));
        // No orgs fetched: the add path falls back to Pro.
        assert_eq!(accounts[0].plan, AccountPlan::Pro);
    }

    #[test]
    fn no_chat_org_rejects_and_writes_nothing() {
        let mut accounts = Vec::new();
        let outcome = apply_session_key(
            &mut accounts,
            "sk",
            &identity("acct-1", Some("a@x.com"), r#"["api"]"#),
            &[],
            || "ID".to_string(),
            0.0,
        );
        assert_eq!(outcome, IntakeOutcome::RejectNoChatOrg);
        assert!(accounts.is_empty());
    }

    #[test]
    fn a_known_identity_is_repaired_in_place() {
        let mut accounts = vec![stored(Some("acct-1"), "a@x.com")];
        let outcome = apply_session_key(
            &mut accounts,
            "sk-fresh",
            &identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#),
            &orgs(r#"["chat","claude_max"]"#),
            || panic!("repair must not mint an id"),
            7.0,
        );
        assert_eq!(
            outcome,
            IntakeOutcome::Updated {
                account_id: "OLD-ID".into(),
                name: "a@x.com".into(),
                old_plan: AccountPlan::Pro,
                new_plan: AccountPlan::Max200,
                warn_no_chat_org: false,
            }
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].status, AccountStatus::Active);
        assert_eq!(accounts[0].last_synced, Some(7.0));
        assert_eq!(
            decrypt_session_key(accounts[0].session_key.as_deref().unwrap()).as_deref(),
            Some("sk-fresh")
        );
    }

    #[test]
    fn a_legacy_record_matched_by_email_gets_its_uuid_backfilled() {
        let mut accounts = vec![stored(None, "A@X.com")];
        apply_session_key(
            &mut accounts,
            "sk",
            &identity("acct-9", Some("a@x.com"), r#"["chat"]"#),
            &[],
            || panic!("repair must not mint an id"),
            0.0,
        );
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].account_uuid.as_deref(), Some("acct-9"));
    }
}
