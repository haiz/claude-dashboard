//! `claude-dashboard-helper pin <id> [--off]` — sets `isPinned` on a stored
//! account.
//!
//! Linux-only, and absent from the usage banner for the same reason `list` and
//! `remove` are. It exists because pinning moved out of a per-consumer setting
//! and into the account store, which is where `contract/account-schema.md`
//! already defines it and where both the extension and the app can see it
//! through `state.json` without opening a second file.

use claude_dashboard_core::model::Account;
use claude_dashboard_core::store;

/// Single-pin semantics: pinning one account clears the rest, so the split
/// changes where pinning is stored without changing what the user sees. The
/// schema itself permits several.
pub fn apply_pin(accounts: &mut [Account], id: &str, on: bool) -> bool {
    if !accounts.iter().any(|a| a.id == id) {
        return false;
    }
    for account in accounts.iter_mut() {
        if account.id == id {
            account.is_pinned = on;
        } else if on {
            account.is_pinned = false;
        }
    }
    true
}

pub fn run_pin(args: &[String]) -> i32 {
    let Some(id) = args.first() else {
        eprintln!("Usage: claude-dashboard-helper pin <id> [--off]");
        return 1;
    };
    let on = !args.iter().any(|a| a == "--off");

    let (mut accounts, _) = match store::load_accounts_for_write() {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("Could not read accounts: {e}");
            return 1;
        }
    };
    if !apply_pin(&mut accounts, id, on) {
        eprintln!("No account with id {id}.");
        return 1;
    }
    if let Err(e) = store::save_accounts(&accounts) {
        eprintln!("Could not save accounts: {e}");
        return 1;
    }
    println!("{} {id}.", if on { "Pinned" } else { "Unpinned" });
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::account;
    use claude_dashboard_core::model::Account;

    fn accounts() -> Vec<Account> {
        ["a", "b", "c"].iter().map(|id| account(id)).collect()
    }

    #[test]
    fn pinning_one_account_sets_its_flag() {
        let mut list = accounts();
        assert!(apply_pin(&mut list, "b", true));
        assert!(list.iter().find(|a| a.id == "b").unwrap().is_pinned);
    }

    #[test]
    fn pinning_clears_every_other_pin_so_the_ui_stays_single_pin() {
        let mut list = accounts();
        list[0].is_pinned = true;
        apply_pin(&mut list, "c", true);
        assert!(!list.iter().find(|a| a.id == "a").unwrap().is_pinned);
        assert!(list.iter().find(|a| a.id == "c").unwrap().is_pinned);
        assert_eq!(list.iter().filter(|a| a.is_pinned).count(), 1);
    }

    #[test]
    fn unpinning_clears_only_that_account() {
        let mut list = accounts();
        list[1].is_pinned = true; // b
        list[2].is_pinned = true; // c
        assert!(apply_pin(&mut list, "b", false));
        assert!(!list.iter().find(|a| a.id == "b").unwrap().is_pinned);
        assert!(list.iter().find(|a| a.id == "c").unwrap().is_pinned);
        assert_eq!(list.iter().filter(|a| a.is_pinned).count(), 1);
    }

    #[test]
    fn an_unknown_id_changes_nothing_and_reports_failure() {
        let mut list = accounts();
        list[0].is_pinned = true;
        assert!(!apply_pin(&mut list, "nope", true));
        assert!(list.iter().find(|a| a.id == "a").unwrap().is_pinned);
    }

    #[test]
    fn no_argument_is_a_usage_error() {
        assert_eq!(run_pin(&[]), 1);
    }
}
