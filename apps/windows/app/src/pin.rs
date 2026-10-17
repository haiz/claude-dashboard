//! Pin writer: a locked load-modify-save of `isPinned`. Pinning leaves exactly
//! one pinned account, as macOS does.

use claude_dashboard_core::store;

/// Pins or unpins `account_id`. Pinning clears the flag on every other account.
#[allow(dead_code)] // wired to the card/account-page pin button in a later task
pub fn set_pinned(account_id: &str, pinned: bool) -> Result<(), String> {
    let _lock = store::lock_store().map_err(|e| e.to_string())?;
    let (mut accounts, _) = store::load_accounts_for_write().map_err(|e| e.to_string())?;
    for a in accounts.iter_mut() {
        if a.id == account_id {
            a.is_pinned = pinned;
        } else if pinned {
            a.is_pinned = false;
        }
    }
    store::save_accounts(&accounts).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::model::Account;

    fn account(id: &str, pinned: bool) -> Account {
        Account::from_json_object(&format!(
            r#"{{"id":"{id}","name":"n","chromeProfilePath":"","plan":"Pro","status":"active","isPinned":{pinned}}}"#
        ))
        .unwrap()
    }

    fn pinned_ids() -> Vec<String> {
        store::load_accounts().unwrap().into_iter().filter(|a| a.is_pinned).map(|a| a.id).collect()
    }

    fn env(dir: &tempfile::TempDir) {
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
    }

    #[test]
    fn pin_unpins_others() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        env(&dir);
        store::save_accounts(&[account("a", true), account("b", true), account("c", false)]).unwrap();
        set_pinned("c", true).unwrap();
        assert_eq!(pinned_ids(), vec!["c".to_string()]);
    }

    #[test]
    fn unpin_clears() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        env(&dir);
        store::save_accounts(&[account("a", true), account("b", false)]).unwrap();
        set_pinned("a", false).unwrap();
        assert!(pinned_ids().is_empty());
    }
}
