//! Claude Desktop's signed-in grant: `oauth:tokenCacheV2` and `lastKnownAccountUuid` in
//! `config.json` (port of macOS `ClaudeDesktopConfigFile.swift`). The cache is one
//! safeStorage blob whose plaintext maps `"<clientId>:<orgId>:<audience>:<scopes>"` to the
//! grant; Desktop rewrites the keys itself on relaunch, so the key written here only has
//! to resolve.

use std::path::PathBuf;

use serde_json::{Map, Value};

use super::safe_storage::{decrypt, encrypt};
use crate::cc_oauth::SCOPE;
use crate::cc_switch::files::write_atomic_json;
use crate::cc_switch::model::{canonical, OAuthCredential};

const CACHE_KEY: &str = "oauth:tokenCacheV2";
const LEGACY_KEY: &str = "oauth:tokenCache";
const UUID_KEY: &str = "lastKnownAccountUuid";
/// Grant fields shared by Desktop's entry and the credential, as (entry key, credential key).
const FIELDS: [(&str, &str); 5] = [
    ("token", "accessToken"),
    ("refreshToken", "refreshToken"),
    ("expiresAt", "expiresAt"),
    ("subscriptionType", "subscriptionType"),
    ("rateLimitTier", "rateLimitTier"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// `config.json` exists but is not a JSON object (nothing was written), or it holds a
    /// grant that will not decrypt or parse.
    Unreadable,
    /// The grant could not be encrypted with the safe-storage key.
    EncryptionFailed,
    Io(String),
}

pub trait DesktopConfig {
    /// The account Desktop is signed in as and its grant; `None` when the file, the cache
    /// or the uuid is absent. Never errors for absence, but `Unreadable` when a present cache
    /// will not decrypt (v20, wrong key, ...) or parse: that may be a live login, so callers
    /// must not overwrite it as if there were none.
    fn read_active_grant(&self) -> Result<Option<(String, OAuthCredential)>, ConfigError>;
    /// Replaces the grant and `lastKnownAccountUuid`, drops the legacy `oauth:tokenCache`,
    /// and keeps every other top-level key. `Unreadable` when an existing file is not a
    /// JSON object (nothing is written).
    fn write_grant(&self, oauth: &OAuthCredential, account_uuid: &str, org_uuid: &str) -> Result<(), ConfigError>;
}

pub struct FileDesktopConfig {
    pub path: PathBuf,
    pub key: [u8; 32],
}

impl DesktopConfig for FileDesktopConfig {
    fn read_active_grant(&self) -> Result<Option<(String, OAuthCredential)>, ConfigError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ConfigError::Unreadable),
        };
        self.parse_grant(&text)
    }

    fn write_grant(&self, oauth: &OAuthCredential, account_uuid: &str, org_uuid: &str) -> Result<(), ConfigError> {
        let mut root = Map::new();
        if self.path.exists() {
            // Present but unreadable (e.g. permissions) must not be treated as absent.
            let text = std::fs::read_to_string(&self.path).map_err(|_| ConfigError::Unreadable)?;
            match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(m)) => root = m,
                _ => return Err(ConfigError::Unreadable),
            }
        }
        let source = oauth.value();
        let mut entry = Map::new();
        for (to, from) in FIELDS {
            if let Some(v) = source.get(from) {
                entry.insert(to.to_string(), v.clone());
            }
        }
        let cache_key = format!("{account_uuid}:{org_uuid}:https://api.anthropic.com:{SCOPE}");
        let mut map = Map::new();
        map.insert(cache_key, Value::Object(entry));
        let plain = canonical(&Value::Object(map)).ok_or(ConfigError::EncryptionFailed)?;
        root.insert(CACHE_KEY.into(), Value::String(encrypt(plain.as_bytes(), &self.key)));
        root.insert(UUID_KEY.into(), Value::String(account_uuid.to_string()));
        root.remove(LEGACY_KEY);
        write_atomic_json(&self.path, &Value::Object(root)).map_err(ConfigError::Io)
    }
}

impl FileDesktopConfig {
    fn parse_grant(&self, text: &str) -> Result<Option<(String, OAuthCredential)>, ConfigError> {
        // A config.json that is not an object holds no grant to lose; writing refuses it anyway.
        let Ok(Value::Object(root)) = serde_json::from_str(text) else { return Ok(None) };
        let Some(uuid) = root.get(UUID_KEY).and_then(Value::as_str) else { return Ok(None) };
        let blob = match root.get(CACHE_KEY) {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::String(b)) if b.trim().is_empty() => return Ok(None),
            Some(Value::String(b)) => b,
            Some(_) => return Err(ConfigError::Unreadable),
        };
        let plain = decrypt(blob, &self.key).ok_or(ConfigError::Unreadable)?;
        let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&plain) else {
            return Err(ConfigError::Unreadable);
        };
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        let Some(entry) = keys.into_iter().find_map(|k| map[k].as_object()) else {
            // An empty cache: signed out.
            return if map.is_empty() { Ok(None) } else { Err(ConfigError::Unreadable) };
        };
        let mut object = Map::new();
        for (from, to) in FIELDS {
            if let Some(v) = entry.get(from) {
                object.insert(to.to_string(), v.clone());
            }
        }
        let oauth = OAuthCredential::from_value(&Value::Object(object)).ok_or(ConfigError::Unreadable)?;
        Ok(Some((uuid.to_string(), oauth)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const KEY: [u8; 32] = [7u8; 32];

    fn sample() -> OAuthCredential {
        OAuthCredential::from_value(&json!({
            "accessToken": "sk-ant-oat01-AT", "refreshToken": "sk-ant-ort01-RT",
            "expiresAt": 1_900_000_000_000_i64, "subscriptionType": "max",
            "rateLimitTier": "default_claude_max_5x",
        }))
        .unwrap()
    }

    fn cfg(dir: &tempfile::TempDir) -> FileDesktopConfig {
        FileDesktopConfig { path: dir.path().join("config.json"), key: KEY }
    }

    fn root(c: &FileDesktopConfig) -> Map<String, Value> {
        serde_json::from_str(&std::fs::read_to_string(&c.path).unwrap()).unwrap()
    }

    fn cache(c: &FileDesktopConfig) -> Map<String, Value> {
        let blob = root(c)[CACHE_KEY].as_str().unwrap().to_string();
        serde_json::from_slice(&decrypt(&blob, &c.key).unwrap()).unwrap()
    }

    #[test]
    fn round_trip_grant() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        c.write_grant(&sample(), "AAA", "ORG").unwrap();
        let (uuid, oauth) = c.read_active_grant().unwrap().unwrap();
        assert_eq!(uuid, "AAA");
        assert_eq!(oauth.refresh_token, "sk-ant-ort01-RT");
        assert_eq!(oauth.expires_at_ms, Some(1_900_000_000_000));
        let v = oauth.value();
        assert_eq!(v["accessToken"], "sk-ant-oat01-AT");
        assert_eq!(v["subscriptionType"], "max");
        assert_eq!(v["rateLimitTier"], "default_claude_max_5x");
    }

    #[test]
    fn entry_uses_desktop_field_names() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        c.write_grant(&sample(), "AAA", "ORG").unwrap();
        let map = cache(&c);
        let entry = map.values().next().unwrap();
        assert_eq!(entry["token"], "sk-ant-oat01-AT");
        assert_eq!(entry["refreshToken"], "sk-ant-ort01-RT");
        assert_eq!(entry["expiresAt"], 1_900_000_000_000_i64);
        assert_eq!(entry["subscriptionType"], "max");
        assert_eq!(entry["rateLimitTier"], "default_claude_max_5x");
        assert!(entry.get("accessToken").is_none());
    }

    #[test]
    fn write_drops_legacy_v1_and_keeps_other_keys() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        std::fs::write(&c.path, r#"{"oauth:tokenCache":"legacy","theme":"dark","nested":{"a":1}}"#).unwrap();
        c.write_grant(&sample(), "AAA", "ORG").unwrap();
        let r = root(&c);
        assert!(r.get("oauth:tokenCache").is_none());
        assert_eq!(r["theme"], "dark");
        assert_eq!(r["nested"]["a"], 1);
        assert_eq!(r["lastKnownAccountUuid"], "AAA");
    }

    #[test]
    fn written_map_key_and_scope() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        c.write_grant(&sample(), "AAA", "ORG").unwrap();
        let keys: Vec<String> = cache(&c).keys().cloned().collect();
        assert_eq!(keys, vec![format!("AAA:ORG:https://api.anthropic.com:{SCOPE}")]);
    }

    #[test]
    fn read_missing_config_is_none() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(cfg(&d).read_active_grant(), Ok(None));
    }

    #[test]
    fn read_with_wrong_key_is_unreadable() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        c.write_grant(&sample(), "AAA", "ORG").unwrap();
        let other = FileDesktopConfig { path: c.path.clone(), key: [9u8; 32] };
        assert_eq!(other.read_active_grant(), Err(ConfigError::Unreadable));
    }

    #[test]
    fn read_v20_blob_is_unreadable() {
        use base64::Engine;
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        let mut raw = b"v20".to_vec();
        raw.extend_from_slice(&[0u8; 40]);
        let blob = base64::engine::general_purpose::STANDARD.encode(raw);
        std::fs::write(&c.path, json!({CACHE_KEY: blob, UUID_KEY: "U"}).to_string()).unwrap();
        assert_eq!(c.read_active_grant(), Err(ConfigError::Unreadable));
    }

    #[test]
    fn read_non_json_plaintext_is_unreadable() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        let blob = encrypt(b"not json", &KEY);
        std::fs::write(&c.path, json!({CACHE_KEY: blob, UUID_KEY: "U"}).to_string()).unwrap();
        assert_eq!(c.read_active_grant(), Err(ConfigError::Unreadable));
    }

    #[test]
    fn read_absent_cache_or_uuid_is_none() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        std::fs::write(&c.path, json!({UUID_KEY: "U", "theme": "dark"}).to_string()).unwrap();
        assert_eq!(c.read_active_grant(), Ok(None));
        std::fs::write(&c.path, json!({CACHE_KEY: "", UUID_KEY: "U"}).to_string()).unwrap();
        assert_eq!(c.read_active_grant(), Ok(None));
        let blob = encrypt(br#"{"k":{"token":"T","refreshToken":"R"}}"#, &KEY);
        std::fs::write(&c.path, json!({CACHE_KEY: blob}).to_string()).unwrap();
        assert_eq!(c.read_active_grant(), Ok(None));
    }

    #[test]
    fn write_into_non_json_file_is_unreadable_and_leaves_file() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        std::fs::write(&c.path, "not json").unwrap();
        assert_eq!(c.write_grant(&sample(), "A", "O"), Err(ConfigError::Unreadable));
        assert_eq!(std::fs::read(&c.path).unwrap(), b"not json");
    }

    #[test]
    fn write_creates_missing_parent_dir() {
        let d = tempfile::tempdir().unwrap();
        let c = FileDesktopConfig { path: d.path().join("Claude").join("config.json"), key: KEY };
        c.write_grant(&sample(), "A", "O").unwrap();
        assert!(c.read_active_grant().unwrap().is_some());
    }

    #[test]
    fn entry_chosen_is_first_by_sorted_key() {
        let d = tempfile::tempdir().unwrap();
        let c = cfg(&d);
        let map = json!({
            "b:org": {"token": "TB", "refreshToken": "RT-B"},
            "a:org": {"token": "TA", "refreshToken": "RT-A"},
        });
        let blob = encrypt(map.to_string().as_bytes(), &KEY);
        std::fs::write(&c.path, json!({CACHE_KEY: blob, UUID_KEY: "U"}).to_string()).unwrap();
        let (uuid, oauth) = c.read_active_grant().unwrap().unwrap();
        assert_eq!(uuid, "U");
        assert_eq!(oauth.refresh_token, "RT-A");
    }
}
