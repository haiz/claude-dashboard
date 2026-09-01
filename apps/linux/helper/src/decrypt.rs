//! `claude-dashboard-helper decrypt` — loads persisted accounts, decrypts
//! each included account's session key, and prints a six-field,
//! sorted-keys JSON projection to stdout.
//!
//! Mirrors `apps/macos/Helper/DecryptCommand.swift`; see
//! `contract/helper-cli.md` "decrypt" for the exact stderr text, inclusion
//! filter, and key-sort-order requirements this reproduces verbatim.

use claude_dashboard_core::model::{Account, AccountStatus};
use claude_dashboard_core::store;
use std::collections::BTreeMap;

/// Runs the `decrypt` subcommand and returns its process exit code.
pub fn run_decrypt() -> i32 {
    // A load error (corrupt accounts.json) is indistinguishable from "no
    // accounts stored" per contract — both take the same stderr path.
    let accounts = store::load_accounts().unwrap_or_default();
    if accounts.is_empty() {
        eprintln!("No accounts found. Run: claude-dashboard-cli sync");
        return 1;
    }

    // Inclusion filter: status == active AND orgId present. sessionKey is
    // deliberately not part of this filter — an included account may still
    // project sessionKey: null.
    let included: Vec<&Account> = accounts
        .iter()
        .filter(|a| a.status == AccountStatus::Active && a.org_id.is_some())
        .collect();
    if included.is_empty() {
        eprintln!("No active accounts with session keys found.");
        return 1;
    }

    // BTreeMap (not a serialized struct) guarantees alphabetical key order
    // in the output regardless of field-declaration order or serde_json's
    // preserve_order feature state — matching Swift's `.sortedKeys`.
    let projected: Vec<BTreeMap<String, serde_json::Value>> = included
        .into_iter()
        .map(|a| {
            // Decrypt on a best-effort basis: fall back to the still
            // -encrypted ciphertext string on any decryption failure, with
            // no error signal to the caller (contract-mandated).
            let session_key = a.session_key.as_ref().map(|cipher| {
                store::decrypt_session_key(cipher).unwrap_or_else(|| cipher.clone())
            });

            let mut m = BTreeMap::new();
            m.insert("name".to_string(), a.name.clone().into());
            m.insert("email".to_string(), a.email.clone().into());
            m.insert("orgId".to_string(), a.org_id.clone().into());
            m.insert("sessionKey".to_string(), session_key.into());
            m.insert(
                "plan".to_string(),
                serde_json::to_value(&a.plan).expect("AccountPlan always serializes to a string"),
            );
            m.insert(
                "status".to_string(),
                serde_json::to_value(&a.status)
                    .expect("AccountStatus always serializes to a string"),
            );
            m
        })
        .collect();

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
