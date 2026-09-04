//! `claude-dashboard-helper add-key` — adds or repairs one account from a
//! session key read on stdin. Never scans a browser.
//!
//! Mirrors `apps/macos/Helper/AddKeyCommand.swift`; `contract/helper-cli.md`
//! "add-key" is the shared stderr and exit-code shape. Which of add and repair
//! happens is `contract/cases/dedupe.json`; what the repair branch may write is
//! `contract/cases/manual-key.json`.
//!
//! The key never reaches stderr, on any branch.

use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use claude_dashboard_core::api::{fetch_account, fetch_organizations, parse_account};
use claude_dashboard_core::identity::{duplicate_index, StoredIdentity};
use claude_dashboard_core::manual_key::{
    manual_key_decision, trimmed_key, ManualKeyDecision, StoredManualTarget,
};
use claude_dashboard_core::model::{Account, AccountSource, AccountStatus, Browser};
use claude_dashboard_core::store;
use uuid::Uuid;

use crate::sync::{parse_orgs, plan_for, plan_wire_value, refreshed_plan_for};

/// 2001-01-01 -> 1970-01-01 offset, so `lastSynced` round-trips to the macOS
/// `Date` reference-date encoding. Same constant `sync` uses.
const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0;

fn now_reference_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
        - REFERENCE_EPOCH_OFFSET
}

pub fn run_add_key() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        eprintln!("No session key on stdin.");
        return 1;
    }
    let Some(session_key) = trimmed_key(&raw) else {
        eprintln!("No session key on stdin.");
        return 1;
    };

    let Some(identity) = fetch_account(session_key).ok().and_then(|b| parse_account(&b)) else {
        eprintln!("Session key not accepted (expired or invalid).");
        return 1;
    };

    let mut accounts = store::load_accounts().unwrap_or_default();
    let stored: Vec<StoredIdentity> = accounts
        .iter()
        .map(|a| StoredIdentity {
            account_uuid: a.account_uuid.clone(),
            email: a.email.clone(),
        })
        .collect();
    let index = duplicate_index(&identity.uuid, identity.email.as_deref(), &stored);

    // A failed fetch parses to an empty slice, which reduces to "no hint" in
    // both `plan_for` (Pro fallback, add path) and `refreshed_plan_for`
    // (leave the stored tier alone, repair path).
    let orgs = fetch_organizations(session_key)
        .ok()
        .map(|body| parse_orgs(&body))
        .unwrap_or_default();

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
        ManualKeyDecision::RejectNoChatOrg => {
            eprintln!("No organization with chat access.");
            1
        }

        ManualKeyDecision::Add { org_id } => {
            let name = identity.email.clone().unwrap_or_else(|| {
                format!("Account {}", &identity.uuid[..8.min(identity.uuid.len())])
            });
            let plan = plan_for(&orgs, &org_id);
            accounts.push(Account {
                id: Uuid::new_v4().to_string().to_uppercase(),
                name: name.clone(),
                email: identity.email.clone(),
                chrome_profile_path: String::new(),
                chrome_profile_name: None,
                org_id: Some(org_id),
                account_uuid: Some(identity.uuid.clone()),
                session_key: Some(store::encrypt_session_key(session_key)),
                browser: Browser::Chrome,
                plan: plan.clone(),
                last_synced: Some(now_reference_seconds()),
                status: AccountStatus::Active,
                is_pinned: false,
                source: AccountSource::Manual,
            });
            if store::save_accounts(&accounts).is_err() {
                eprintln!("Could not write the account store.");
                return 1;
            }
            eprintln!("Added: {name} ({})", plan_wire_value(&plan));
            0
        }

        ManualKeyDecision::Repair {
            writes,
            warn_no_chat_org,
        } => {
            let Some(i) = index else { return 1 };
            let old_plan = accounts[i].plan.clone();
            accounts[i].session_key = Some(store::encrypt_session_key(session_key));
            accounts[i].status = AccountStatus::Active;
            accounts[i].last_synced = Some(now_reference_seconds());
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
            if let Some(plan) = refreshed_plan_for(&accounts[i], &orgs) {
                accounts[i].plan = plan;
            }
            if store::save_accounts(&accounts).is_err() {
                eprintln!("Could not write the account store.");
                return 1;
            }

            let name = accounts[i]
                .email
                .clone()
                .unwrap_or_else(|| accounts[i].name.clone());
            eprintln!("Updated key: {name}");
            if accounts[i].plan != old_plan {
                eprintln!(
                    "Updated plan: {name} ({} -> {})",
                    plan_wire_value(&old_plan),
                    plan_wire_value(&accounts[i].plan)
                );
            }
            // The resolve result, not the stored value: an account that kept a
            // stored org_id but lost chat access still polls a dead org.
            if warn_no_chat_org {
                eprintln!("Warning: no organization with chat access; usage will not update.");
            }
            0
        }
    }
}
