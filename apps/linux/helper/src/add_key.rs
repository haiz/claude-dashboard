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
use claude_dashboard_core::key_intake::{apply_session_key, IntakeOutcome};
use claude_dashboard_core::manual_key::trimmed_key;
use claude_dashboard_core::plan::{parse_orgs, plan_wire_value};
use claude_dashboard_core::store;
use uuid::Uuid;

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

    let (mut accounts, quarantined) = match store::load_accounts_for_write() {
        Ok(loaded) => loaded,
        Err(e) => {
            eprintln!("Could not read the account store: {e}");
            return 1;
        }
    };
    if let Some(kept) = &quarantined {
        eprintln!(
            "Could not read the account store. The unreadable copy is kept at {}; this run starts from no accounts.",
            kept.display()
        );
    }
    // A failed fetch parses to an empty slice, which reduces to "no hint" in
    // both the add path (Pro fallback) and the repair path (leave the stored
    // tier alone).
    let orgs = fetch_organizations(session_key)
        .ok()
        .map(|body| parse_orgs(&body))
        .unwrap_or_default();

    let outcome = apply_session_key(
        &mut accounts,
        session_key,
        &identity,
        &orgs,
        || Uuid::new_v4().to_string().to_uppercase(),
        now_reference_seconds(),
    );

    match outcome {
        IntakeOutcome::RejectNoChatOrg => {
            eprintln!("No organization with chat access.");
            1
        }

        IntakeOutcome::Added { name, plan, .. } => {
            if store::save_accounts(&accounts).is_err() {
                eprintln!("Could not write the account store.");
                return 1;
            }
            eprintln!("Added: {name} ({})", plan_wire_value(&plan));
            0
        }

        IntakeOutcome::Updated {
            name,
            old_plan,
            new_plan,
            warn_no_chat_org,
            ..
        } => {
            if store::save_accounts(&accounts).is_err() {
                eprintln!("Could not write the account store.");
                return 1;
            }
            eprintln!("Updated key: {name}");
            if new_plan != old_plan {
                eprintln!(
                    "Updated plan: {name} ({} -> {})",
                    plan_wire_value(&old_plan),
                    plan_wire_value(&new_plan)
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
