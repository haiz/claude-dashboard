//! Settings > Accounts glue: delete an account (muting its extension install)
//! and manage muted sources. Plain blocking functions; the UI calls them from
//! a worker thread because they do store I/O under the store lock.

use claude_dashboard_core::{extension_sources, store};

/// Removes an account by id. If an extension install was bound to it, that
/// install is muted so the extension cannot silently re-add it. One store lock
/// is held across both writes. Returns the muted installId, if any.
pub fn delete_account(account_id: &str) -> Result<Option<String>, String> {
    let _lock = store::lock_store().map_err(|e| e.to_string())?;
    let (mut accounts, _) = store::load_accounts_for_write().map_err(|e| e.to_string())?;
    let before = accounts.len();
    accounts.retain(|a| a.id != account_id);
    if accounts.len() == before {
        return Ok(None);
    }
    store::save_accounts(&accounts).map_err(|e| e.to_string())?;

    let path = extension_sources::sources_path();
    let mut sources = extension_sources::load(&path)?;
    let Some(install_id) = sources
        .bindings
        .iter()
        .find(|(_, b)| b.account_id == account_id)
        .map(|(id, _)| id.clone())
    else {
        return Ok(None);
    };
    sources.muted.insert(install_id.clone());
    extension_sources::save(&path, &sources)?;
    Ok(Some(install_id))
}

pub fn unmute(install_id: &str) -> Result<(), String> {
    let _lock = store::lock_store().map_err(|e| e.to_string())?;
    let path = extension_sources::sources_path();
    let mut sources = extension_sources::load(&path)?;
    if sources.muted.remove(install_id) {
        extension_sources::save(&path, &sources)?;
    }
    Ok(())
}

/// installIds currently muted.
pub fn muted_sources() -> Vec<String> {
    extension_sources::load(&extension_sources::sources_path())
        .map(|s| s.muted.into_iter().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::extension_sources::{load, save, sources_path, ExtensionSources};
    use claude_dashboard_core::model::Account;

    fn account(id: &str) -> Account {
        Account::from_json_object(&format!(
            r#"{{"id":"{id}","name":"n","chromeProfilePath":"","plan":"Pro","status":"active"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn delete_mutes_the_extension_install() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());

        store::save_accounts(&[account("ACC-1"), account("ACC-2")]).unwrap();
        let mut s = ExtensionSources::default();
        s.bind("inst-1", "ACC-1", "edge");
        save(&sources_path(), &s).unwrap();

        assert_eq!(delete_account("ACC-1").unwrap(), Some("inst-1".to_string()));
        let ids: Vec<String> = store::load_accounts().unwrap().into_iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["ACC-2".to_string()]);
        assert!(load(&sources_path()).unwrap().is_muted("inst-1"));
        assert_eq!(muted_sources(), vec!["inst-1".to_string()]);

        unmute("inst-1").unwrap();
        assert!(!load(&sources_path()).unwrap().is_muted("inst-1"));
        assert!(muted_sources().is_empty());

        // A non-extension account has no binding to mute.
        assert_eq!(delete_account("ACC-2").unwrap(), None);
        assert!(store::load_accounts().unwrap().is_empty());
    }
}
