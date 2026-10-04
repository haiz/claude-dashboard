//! Add-account glue for pasted and scanned session keys. Plain blocking
//! functions: the UI calls them from a worker thread, never the UI thread.
//!
//! Order mirrors the bridge handler: validate (`/api/account`) -> fetch orgs
//! -> lock -> load -> `apply_session_key` -> save. The session key never
//! appears in an [`AddOutcome`].

use std::time::{SystemTime, UNIX_EPOCH};

use claude_dashboard_core::api::{fetch_account, fetch_organizations, parse_account};
use claude_dashboard_core::key_intake::{apply_session_key, IntakeOutcome};
use claude_dashboard_core::manual_key::trimmed_key;
use claude_dashboard_core::plan::parse_orgs;
use claude_dashboard_core::scan::ScannedSession;
use claude_dashboard_core::store;
use uuid::Uuid;

/// Reference-date (2001-01-01) offset from the Unix epoch, as `last_synced` uses.
const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0;

#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    Added(String),
    Updated(String),
    Rejected(String),
    NoChatOrg,
    StoreError(String),
}

/// Pure mapping from the shared intake outcome to the UI-facing one.
fn map_outcome(outcome: IntakeOutcome) -> AddOutcome {
    match outcome {
        IntakeOutcome::Added { name, .. } => AddOutcome::Added(name),
        IntakeOutcome::Updated { name, .. } => AddOutcome::Updated(name),
        IntakeOutcome::RejectNoChatOrg => AddOutcome::NoChatOrg,
    }
}

fn now_reference() -> f64 {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    unix - REFERENCE_EPOCH_OFFSET
}

/// Validates a pasted/scanned key over the network, then adds or repairs the
/// account under the store lock. An unaccepted key writes nothing.
pub fn add_from_session_key(session_key: &str) -> AddOutcome {
    let Some(key) = trimmed_key(session_key) else {
        return AddOutcome::Rejected("no key".into());
    };
    let Some(identity) = fetch_account(key).ok().and_then(|b| parse_account(&b)) else {
        return AddOutcome::Rejected("Session key not accepted".into());
    };
    let orgs = fetch_organizations(key)
        .ok()
        .map(|b| parse_orgs(&b))
        .unwrap_or_default();

    let _lock = match store::lock_store() {
        Ok(l) => l,
        Err(e) => return AddOutcome::StoreError(e.to_string()),
    };
    let mut accounts = match store::load_accounts_for_write() {
        Ok((a, _)) => a,
        Err(e) => return AddOutcome::StoreError(e.to_string()),
    };
    let outcome = apply_session_key(
        &mut accounts,
        key,
        &identity,
        &orgs,
        || Uuid::new_v4().to_string().to_uppercase(),
        now_reference(),
    );
    if outcome == IntakeOutcome::RejectNoChatOrg {
        return AddOutcome::NoChatOrg;
    }
    if let Err(e) = store::save_accounts(&accounts) {
        return AddOutcome::StoreError(e.to_string());
    }
    map_outcome(outcome)
}

/// One outcome per scanned session, in order.
pub fn add_scanned(sessions: &[ScannedSession]) -> Vec<AddOutcome> {
    sessions
        .iter()
        .map(|s| add_from_session_key(&s.session_key))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::model::AccountPlan;

    #[test]
    fn map_outcome_maps_each_variant() {
        assert_eq!(
            map_outcome(IntakeOutcome::Added {
                account_id: "i".into(),
                name: "a@x.com".into(),
                plan: AccountPlan::Pro
            }),
            AddOutcome::Added("a@x.com".into())
        );
        assert_eq!(
            map_outcome(IntakeOutcome::Updated {
                account_id: "i".into(),
                name: "b@x.com".into(),
                old_plan: AccountPlan::Pro,
                new_plan: AccountPlan::Pro,
                warn_no_chat_org: false
            }),
            AddOutcome::Updated("b@x.com".into())
        );
        assert_eq!(map_outcome(IntakeOutcome::RejectNoChatOrg), AddOutcome::NoChatOrg);
    }

    #[test]
    fn empty_key_is_rejected() {
        assert_eq!(add_from_session_key("   "), AddOutcome::Rejected("no key".into()));
    }

    #[test]
    fn paste_rejects_unaccepted_key_without_writing() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        // Closed port: the validation fetch fails, so the key is "not accepted".
        // Serialised with other env-mutating tests via testenv::lock.
        std::env::set_var("CLAUDE_DASHBOARD_API_BASE", "http://127.0.0.1:1");
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());

        let out = add_from_session_key("sk-bad");

        assert!(matches!(out, AddOutcome::Rejected(_)), "got {out:?}");
        assert!(
            !claude_dashboard_core::store::accounts_path().exists(),
            "a rejected key must not create accounts.json"
        );
        if let AddOutcome::Rejected(m) = out {
            assert!(!m.contains("sk-bad"));
        }
    }
}
