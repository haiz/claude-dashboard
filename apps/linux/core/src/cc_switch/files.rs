//! Where Claude Code's login lives on disk, and the app's per-account vault.
//!
//! - The **slot**: Claude Code's credentials file (`<config dir>\.credentials.json` on
//!   Windows; macOS keeps the same JSON in the Keychain). Only `claudeAiOauth` is replaced;
//!   every other key (`mcpOAuth`, …) survives.
//! - The **config**: `.claude.json`; only `oauthAccount` is replaced.
//! - The **vault**: one sealed file per dashboard account.
//!
//! Writes go to a temp file in the same directory, then rename over the target.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::model::{canonical, OAuthCredential, VaultEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotError {
    /// The file exists but is not a JSON object; nothing was written.
    Unreadable,
    /// `claudeAiOauth` is no longer what the caller read; nothing was written.
    ChangedSinceRead,
    Io(String),
}

/// The credential Claude Code is using right now.
pub trait CredentialSlot {
    /// `None` when the file or its `claudeAiOauth` is missing.
    fn read_oauth(&self) -> Result<Option<OAuthCredential>, SlotError>;
    /// Replaces `claudeAiOauth` (keeping every other key) only while the file still holds
    /// `expecting`; otherwise `ChangedSinceRead` and nothing is written.
    fn write_oauth(&self, c: &OAuthCredential, expecting: Option<&OAuthCredential>) -> Result<(), SlotError>;
}

/// The `oauthAccount` key of `.claude.json`.
pub trait ConfigFile {
    /// Canonical JSON of `oauthAccount`; `None` when the file or key is missing.
    fn read_oauth_account(&self) -> Option<String>;
    /// Replaces `oauthAccount`, keeping every other key. A missing or non-object file is an error.
    fn write_oauth_account(&self, json: &str) -> Result<(), String>;
}

/// The app's per-account copies of Claude Code logins.
pub trait Vault {
    fn load(&self, id: &str) -> Option<VaultEntry>;
    fn save(&self, id: &str, e: &VaultEntry) -> Result<(), String>;
    /// A missing copy is not an error.
    fn delete(&self, id: &str);
}

/// `(credentials file, .claude.json)` as Claude Code resolves them: under
/// `CLAUDE_CONFIG_DIR` when set, else `<home>\.claude\.credentials.json` and `<home>\.claude.json`.
pub fn paths_from(config_dir: Option<&str>, home: Option<&str>) -> (PathBuf, PathBuf) {
    if let Some(dir) = config_dir.filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        return (dir.join(".credentials.json"), dir.join(".claude.json"));
    }
    let home = PathBuf::from(home.unwrap_or_default());
    (home.join(".claude").join(".credentials.json"), home.join(".claude.json"))
}

/// The JSON object in `path`: `Ok(None)` when the file is missing, `Err(Unreadable)` when it
/// is not a JSON object.
fn read_object(path: &Path) -> Result<Option<serde_json::Map<String, Value>>, SlotError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(SlotError::Io(e.to_string())),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok(Some(m)),
        _ => Err(SlotError::Unreadable),
    }
}

/// Writes `v` (pretty) to `path` through a temp file in the same directory.
pub fn write_atomic_json(path: &Path, v: &Value) -> Result<(), String> {
    write_atomic(path, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?.as_bytes())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent directory")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".{name}.{}.{nanos}.tmp", std::process::id()));
    let result = std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    result.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub struct FileSlot {
    pub path: PathBuf,
}

impl CredentialSlot for FileSlot {
    fn read_oauth(&self) -> Result<Option<OAuthCredential>, SlotError> {
        Ok(read_object(&self.path)?.and_then(|m| m.get("claudeAiOauth").and_then(OAuthCredential::from_value)))
    }

    /// The compare re-reads the file right before the write: this narrows the window in which
    /// a concurrent refresh is lost to that gap (macOS has the same two-step compare).
    fn write_oauth(&self, c: &OAuthCredential, expecting: Option<&OAuthCredential>) -> Result<(), SlotError> {
        let mut root = read_object(&self.path)?.unwrap_or_default();
        let current = root.get("claudeAiOauth").and_then(canonical);
        if current.as_deref() != expecting.map(|e| e.json.as_str()) {
            return Err(SlotError::ChangedSinceRead);
        }
        root.insert("claudeAiOauth".into(), c.value());
        write_atomic_json(&self.path, &Value::Object(root)).map_err(SlotError::Io)
    }
}

pub struct FileConfig {
    pub path: PathBuf,
}

impl ConfigFile for FileConfig {
    fn read_oauth_account(&self) -> Option<String> {
        read_object(&self.path).ok().flatten()?.get("oauthAccount").and_then(canonical)
    }

    fn write_oauth_account(&self, json: &str) -> Result<(), String> {
        let account: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if !account.is_object() {
            return Err("oauthAccount is not an object".into());
        }
        let mut root = match read_object(&self.path) {
            Ok(Some(m)) => m,
            _ => return Err(format!("{} is missing or not a JSON object", self.path.display())),
        };
        root.insert("oauthAccount".into(), account);
        write_atomic_json(&self.path, &Value::Object(root))
    }
}

/// One file per account, `<dir>\<id>.bin`, holding the entry's JSON sealed by `seal`.
pub struct FileVault {
    pub dir: PathBuf,
    pub seal: fn(&[u8]) -> Option<Vec<u8>>,
    pub open: fn(&[u8]) -> Option<Vec<u8>>,
}

impl FileVault {
    /// Sealed to the current Windows user with DPAPI, like the session keys.
    #[cfg(windows)]
    pub fn dpapi(dir: PathBuf) -> Self {
        FileVault { dir, seal: crate::userprotect::protect, open: crate::userprotect::unprotect }
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.bin"))
    }
}

impl Vault for FileVault {
    fn load(&self, id: &str) -> Option<VaultEntry> {
        let sealed = std::fs::read(self.file(id)).ok()?;
        let plain = (self.open)(&sealed)?;
        VaultEntry::from_json(std::str::from_utf8(&plain).ok()?)
    }

    fn save(&self, id: &str, e: &VaultEntry) -> Result<(), String> {
        let sealed = (self.seal)(e.to_json().as_bytes()).ok_or("could not seal the vault entry")?;
        write_atomic(&self.file(id), &sealed)
    }

    fn delete(&self, id: &str) {
        let _ = std::fs::remove_file(self.file(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cred(refresh: &str) -> OAuthCredential {
        OAuthCredential::from_value(&json!({"accessToken": "at", "refreshToken": refresh, "expiresAt": 5})).unwrap()
    }

    fn plain_vault(dir: &Path) -> FileVault {
        fn id(b: &[u8]) -> Option<Vec<u8>> {
            Some(b.to_vec())
        }
        FileVault { dir: dir.to_path_buf(), seal: id, open: id }
    }

    fn read(p: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn slot_write_keeps_other_keys() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".credentials.json");
        std::fs::write(&p, json!({"claudeAiOauth": cred("old").value(), "mcpOAuth": {"s": 1}, "futureKey": 1}).to_string()).unwrap();
        let slot = FileSlot { path: p.clone() };
        let before = slot.read_oauth().unwrap();
        assert_eq!(before, Some(cred("old")));
        slot.write_oauth(&cred("new"), before.as_ref()).unwrap();
        let v = read(&p);
        assert_eq!(v["claudeAiOauth"]["refreshToken"], "new");
        assert_eq!(v["mcpOAuth"], json!({"s": 1}));
        assert_eq!(v["futureKey"], 1);
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1, "no temp litter");
    }

    #[test]
    fn slot_missing_file_is_none_then_created() {
        let d = tempfile::tempdir().unwrap();
        let slot = FileSlot { path: d.path().join("sub").join(".credentials.json") };
        assert_eq!(slot.read_oauth(), Ok(None));
        slot.write_oauth(&cred("x"), None).unwrap();
        assert_eq!(slot.read_oauth().unwrap(), Some(cred("x")));
    }

    #[test]
    fn slot_rejects_non_object() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".credentials.json");
        std::fs::write(&p, "[1]").unwrap();
        let slot = FileSlot { path: p.clone() };
        assert_eq!(slot.read_oauth(), Err(SlotError::Unreadable));
        assert_eq!(slot.write_oauth(&cred("x"), None), Err(SlotError::Unreadable));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "[1]");
    }

    #[test]
    fn slot_write_refuses_when_changed() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".credentials.json");
        std::fs::write(&p, json!({"claudeAiOauth": cred("refreshed").value()}).to_string()).unwrap();
        let slot = FileSlot { path: p.clone() };
        assert_eq!(slot.write_oauth(&cred("target"), Some(&cred("stale"))), Err(SlotError::ChangedSinceRead));
        assert_eq!(read(&p)["claudeAiOauth"]["refreshToken"], "refreshed");
        assert_eq!(slot.write_oauth(&cred("target"), None), Err(SlotError::ChangedSinceRead), "expected absent");
    }

    #[test]
    fn config_replaces_only_oauth_account() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(".claude.json");
        std::fs::write(&p, json!({"oauthAccount": {"emailAddress": "a@x.com"}, "projects": {"k": 1}}).to_string()).unwrap();
        let c = FileConfig { path: p.clone() };
        assert_eq!(c.read_oauth_account().as_deref(), Some(r#"{"emailAddress":"a@x.com"}"#));
        c.write_oauth_account(r#"{"emailAddress":"b@x.com","accountUuid":"u"}"#).unwrap();
        let v = read(&p);
        assert_eq!(v["oauthAccount"]["emailAddress"], "b@x.com");
        assert_eq!(v["projects"], json!({"k": 1}));
        assert!(c.write_oauth_account("[1]").is_err());
    }

    #[test]
    fn config_missing_file_is_error() {
        let d = tempfile::tempdir().unwrap();
        let c = FileConfig { path: d.path().join(".claude.json") };
        assert_eq!(c.read_oauth_account(), None);
        assert!(c.write_oauth_account("{}").is_err());
        assert!(!d.path().join(".claude.json").exists());
    }

    #[test]
    fn vault_round_trip_and_delete() {
        let d = tempfile::tempdir().unwrap();
        let v = plain_vault(d.path());
        let e = VaultEntry { oauth: cred("rt-secret"), oauth_account: canonical(&json!({"emailAddress": "a@x.com"})).unwrap() };
        assert_eq!(v.load("a"), None);
        v.save("a", &e).unwrap();
        assert_eq!(v.load("a"), Some(e));
        v.delete("a");
        v.delete("a");
        assert_eq!(v.load("a"), None);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_vault_hides_the_token() {
        let d = tempfile::tempdir().unwrap();
        let v = FileVault::dpapi(d.path().to_path_buf());
        let e = VaultEntry { oauth: cred("rt-secret-xyz"), oauth_account: canonical(&json!({"emailAddress": "a@x.com"})).unwrap() };
        v.save("a", &e).unwrap();
        let raw = std::fs::read(d.path().join("a.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("rt-secret-xyz"));
        assert_eq!(v.load("a"), Some(e));
    }

    #[test]
    fn paths_follow_claude_config_dir() {
        let (c, j) = paths_from(None, Some(r"C:\Users\u"));
        assert_eq!(c, Path::new(r"C:\Users\u").join(".claude").join(".credentials.json"));
        assert_eq!(j, Path::new(r"C:\Users\u").join(".claude.json"));
        let (c, j) = paths_from(Some(r"D:\cc"), Some(r"C:\Users\u"));
        assert_eq!(c, Path::new(r"D:\cc").join(".credentials.json"));
        assert_eq!(j, Path::new(r"D:\cc").join(".claude.json"));
        let (c, _) = paths_from(Some(""), Some("h"));
        assert_eq!(c, Path::new("h").join(".claude").join(".credentials.json"), "empty means unset");
    }
}
