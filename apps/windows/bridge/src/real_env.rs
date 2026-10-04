//! The production [`Environment`]: real network, store and named-pipe notify.
//! The network and store calls are the same `core` functions the Linux
//! helper's `add-key` uses, so this is largely a thin pass-through.

use std::time::{SystemTime, UNIX_EPOCH};

use claude_dashboard_core::api::{fetch_account, fetch_organizations, parse_account, ParsedAccount};
use claude_dashboard_core::model::Account;
use claude_dashboard_core::plan::{parse_orgs, ParsedOrg};
use claude_dashboard_core::store;
use uuid::Uuid;

use crate::handler::Environment;
use crate::notify;
use crate::sources::{self, ExtensionSources};

/// 2001-01-01 -> 1970-01-01, so `lastSynced` round-trips to the macOS `Date`
/// reference-date encoding. Same constant the helper uses.
const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0;

pub struct RealEnv;

impl Environment for RealEnv {
    type Guard = store::StoreLock;

    fn fetch_account(&self, session_key: &str) -> Option<ParsedAccount> {
        fetch_account(session_key).ok().and_then(|b| parse_account(&b))
    }

    fn fetch_orgs(&self, session_key: &str) -> Vec<ParsedOrg> {
        fetch_organizations(session_key)
            .ok()
            .map(|b| parse_orgs(&b))
            .unwrap_or_default()
    }

    fn lock(&self) -> Result<Self::Guard, String> {
        store::lock_store().map_err(|e| e.to_string())
    }

    fn load_accounts(&self) -> Result<Vec<Account>, String> {
        store::load_accounts().map_err(|e| e.to_string())
    }

    fn save_accounts(&self, accounts: &[Account]) -> Result<(), String> {
        store::save_accounts(accounts).map_err(|e| e.to_string())
    }

    fn load_sources(&self) -> Result<ExtensionSources, String> {
        sources::load(&sources::sources_path())
    }

    fn save_sources(&self, s: &ExtensionSources) -> Result<(), String> {
        sources::save(&sources::sources_path(), s)
    }

    fn new_id(&self) -> String {
        Uuid::new_v4().to_string().to_uppercase()
    }

    fn now_reference(&self) -> f64 {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        unix - REFERENCE_EPOCH_OFFSET
    }

    fn notify_app(&self) {
        notify::reload();
    }
}
