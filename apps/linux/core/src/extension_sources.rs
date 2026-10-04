//! `extension-sources.json`, beside `accounts.json`: which browser-extension
//! install feeds which account, and which installs the user muted by deleting
//! their account. Kept out of `accounts.json` so the cross-platform account
//! schema stays unchanged. Shape: `contract/windows.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionSources {
    /// `installId` -> the account it feeds.
    #[serde(default)]
    pub bindings: BTreeMap<String, Binding>,
    /// `installId`s whose account the user deleted; the host ignores their keys.
    #[serde(default)]
    pub muted: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    #[serde(rename = "accountId")]
    pub account_id: String,
    pub browser: String,
}

impl ExtensionSources {
    pub fn bind(&mut self, install_id: &str, account_id: &str, browser: &str) {
        self.bindings.insert(
            install_id.to_string(),
            Binding {
                account_id: account_id.to_string(),
                browser: browser.to_string(),
            },
        );
    }

    pub fn is_muted(&self, install_id: &str) -> bool {
        self.muted.contains(install_id)
    }
}

/// `extension-sources.json` beside the account store.
pub fn sources_path() -> PathBuf {
    store::accounts_path().with_file_name("extension-sources.json")
}

/// Loads the sources. A missing file is not an error — it means nothing has
/// been bound or muted yet. A parse failure *is* an error: a corrupt file must
/// never silently read as "no mutes".
pub fn load(path: &Path) -> Result<ExtensionSources, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ExtensionSources::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Saves the sources by temp-file-then-rename, like `store::save_accounts`.
/// Callers hold `store::lock_store()` across their read-modify-write.
pub fn save(path: &Path, sources: &ExtensionSources) -> Result<(), String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| fail(&e))?;
    }
    let json = serde_json::to_string_pretty(sources).map_err(|e| fail(&e))?;
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&tmp, json).map_err(|e| fail(&e))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        fail(&e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_empty() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            load(&d.path().join("nope.json")).unwrap(),
            ExtensionSources::default()
        );
    }

    #[test]
    fn bindings_and_mutes_round_trip_in_the_documented_shape() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("sub").join("extension-sources.json");
        let mut s = ExtensionSources::default();
        s.bind("inst-1", "ACC-1", "edge");
        s.muted.insert("inst-2".into());
        save(&path, &s).unwrap();

        assert_eq!(load(&path).unwrap(), s);
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            raw,
            serde_json::json!({
                "bindings": { "inst-1": { "accountId": "ACC-1", "browser": "edge" } },
                "muted": ["inst-2"]
            })
        );
    }

    #[test]
    fn rebinding_an_install_moves_it() {
        let mut s = ExtensionSources::default();
        s.bind("inst-1", "ACC-1", "chrome");
        s.bind("inst-1", "ACC-2", "chrome");
        assert_eq!(s.bindings["inst-1"].account_id, "ACC-2");
        assert!(!s.is_muted("inst-1"));
    }

    #[test]
    fn an_unparseable_file_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("extension-sources.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(
            load(&path).is_err(),
            "a corrupt file must not silently unmute everything"
        );
    }
}
