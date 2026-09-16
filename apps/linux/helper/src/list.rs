//! `claude-dashboard-helper list` — prints every stored account, with no
//! session keys, for a UI that needs to manage accounts rather than poll them.
//!
//! `decrypt` cannot serve this purpose and must not be widened to: its
//! six-field projection and its `status == active && orgId.is_some()`
//! inclusion filter are pinned by `contract/helper-cli.md`, and that filter is
//! exactly what hides the expired accounts an account-management UI exists to
//! fix. `list` is therefore a separate command with a separate contract:
//! every account, every field a UI needs, and deliberately no secret.

use claude_dashboard_core::model::Account;
use claude_dashboard_core::store;
use std::collections::BTreeMap;

/// The per-account projection. Keys are alphabetical for the same reason
/// `decrypt`'s are: a stable, diffable output that does not depend on struct
/// declaration order.
///
/// `sessionKey` is absent by construction, not filtered out at the end — the
/// map is built from named fields, so a future field added to `Account`
/// cannot leak here by accident.
fn project(account: &Account) -> BTreeMap<String, serde_json::Value> {
    let mut m = BTreeMap::new();
    m.insert("id".to_string(), account.id.clone().into());
    m.insert("name".to_string(), account.name.clone().into());
    m.insert("email".to_string(), account.email.clone().into());
    m.insert("orgId".to_string(), account.org_id.clone().into());
    m.insert(
        "chromeProfileName".to_string(),
        account.chrome_profile_name.clone().into(),
    );
    m.insert(
        "browser".to_string(),
        serde_json::to_value(&account.browser).expect("Browser always serializes to a string"),
    );
    m.insert(
        "plan".to_string(),
        serde_json::to_value(&account.plan).expect("AccountPlan always serializes to a string"),
    );
    m.insert(
        "status".to_string(),
        serde_json::to_value(&account.status).expect("AccountStatus always serializes to a string"),
    );
    m.insert(
        "source".to_string(),
        serde_json::to_value(&account.source).expect("AccountSource always serializes to a string"),
    );
    m.insert("isPinned".to_string(), account.is_pinned.into());
    // Unix seconds, so a caller need not know about the reference-date offset
    // the stored value uses.
    m.insert("lastSynced".to_string(), match account.last_synced_unix() {
        Some(secs) => serde_json::json!(secs),
        None => serde_json::Value::Null,
    });
    m
}

pub fn run_list() -> i32 {
    // A load error is indistinguishable from "no accounts stored", exactly as
    // in `decrypt` — but unlike `decrypt`, an empty store is not an error
    // here: "you have no accounts" is the very thing the caller is asking
    // about, so it prints `[]` and exits 0.
    let accounts = store::load_accounts().unwrap_or_default();
    let projected: Vec<BTreeMap<String, serde_json::Value>> =
        accounts.iter().map(project).collect();

    match serde_json::to_string_pretty(&projected) {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(_) => {
            eprintln!("Failed to encode accounts.");
            1
        }
    }
}

/// `claude-dashboard-helper remove <id>` — deletes one stored account.
///
/// The argument is the `id` field `list` prints, not an email: two accounts
/// can share an email across organisations, and `id` is the only value the
/// store guarantees is unique.
pub fn run_remove(args: &[String]) -> i32 {
    let Some(id) = args.first() else {
        eprintln!("Usage: claude-dashboard-helper remove <id>");
        return 1;
    };

    let (mut accounts, _) = match store::load_accounts_for_write() {
        Ok(loaded) => loaded,
        Err(_) => {
            eprintln!("Could not read the account store.");
            return 1;
        }
    };

    let before = accounts.len();
    accounts.retain(|a| &a.id != id);
    if accounts.len() == before {
        // Naming a missing account is a caller mistake worth reporting, not a
        // silent success: a UI that deleted the wrong row should find out.
        eprintln!("No account with id {id}.");
        return 1;
    }

    if store::save_accounts(&accounts).is_err() {
        eprintln!("Could not write the account store.");
        return 1;
    }
    println!("Removed {id}.");
    0
}
