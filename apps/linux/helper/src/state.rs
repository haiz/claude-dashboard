//! Writes `state.json`, the daemon's snapshot for the GNOME Shell extension
//! and the GTK app. Shape and rules: `contract/linux-state.md`.
//!
//! The daemon records observations and holds no policy: usage payloads are
//! copied through verbatim, and burn rate, sort order and the panel row are
//! left to `apps/linux/lib/` on the consumer side.

use claude_dashboard_core::model::Account;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u64 = 1;

pub fn state_path() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local")
                .join("share")
        });
    base.join("claude-dashboard").join("state.json")
}

/// Built from named fields rather than by serializing `Account`, so a field
/// added to the store — `sessionKey` above all — cannot leak into a file two
/// unprivileged readers watch.
fn project(account: &Account) -> Value {
    json!({
        "id": account.id,
        "name": account.name,
        "email": account.email,
        "orgId": account.org_id,
        "chromeProfileName": account.chrome_profile_name,
        "plan": account.plan,
        "status": account.status,
        "isPinned": account.is_pinned,
        "source": account.source,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn build_state(
    accounts: &[Account],
    usage: &BTreeMap<String, Value>,
    errors: &BTreeMap<String, String>,
    active_email: Option<&str>,
    polled_at_ms: i64,
    interval_seconds: u64,
    fatal: Option<&str>,
) -> Value {
    let mut usage_map = Map::new();
    for (id, payload) in usage {
        usage_map.insert(id.clone(), payload.clone());
    }
    let mut error_map = Map::new();
    for (id, message) in errors {
        error_map.insert(id.clone(), Value::String(message.clone()));
    }

    json!({
        "schemaVersion": SCHEMA_VERSION,
        "polledAtMs": polled_at_ms,
        "daemon": {
            "version": env!("CARGO_PKG_VERSION"),
            "intervalSeconds": interval_seconds,
        },
        "activeClaudeCodeEmail": active_email,
        "accounts": accounts.iter().map(project).collect::<Vec<_>>(),
        "usage": Value::Object(usage_map),
        "errors": Value::Object(error_map),
        "fatal": fatal,
    })
}

/// Rule 1 of `contract/linux-state.md`: a `Gio.FileMonitor` consumer must
/// never observe a partial document, so the bytes land under a sibling name
/// and are moved into place by `rename`, which is atomic within a filesystem.
pub fn write_atomic(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(serde_json::to_string(value)?.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::account;

    #[test]
    fn the_document_declares_schema_version_one() {
        let v = build_state(&[], &BTreeMap::new(), &BTreeMap::new(), None, 5, 120, None);
        assert_eq!(v["schemaVersion"], 1);
        assert_eq!(v["polledAtMs"], 5);
        assert_eq!(v["daemon"]["intervalSeconds"], 120);
    }

    #[test]
    fn accounts_carry_the_store_id_and_its_pin_state() {
        let mut a = account("acc-1");
        a.is_pinned = true;
        let v = build_state(&[a], &BTreeMap::new(), &BTreeMap::new(), None, 0, 120, None);
        assert_eq!(v["accounts"][0]["id"], "acc-1");
        assert_eq!(v["accounts"][0]["isPinned"], true);
        assert_eq!(v["accounts"][0]["orgId"], "org-acc-1");
        // MF-2: `source` is part of the spec's state.json shape
        // (docs/superpowers/specs/2026-09-16-linux-process-split-design.md
        // line 140) and lib/model.js:75 reads it — a manually-added account
        // must not lose its "pasted key" affordance once consumers read
        // state.json instead of `decrypt`.
        assert_eq!(v["accounts"][0]["source"], "browser");
    }

    #[test]
    fn the_session_key_never_reaches_the_state_file() {
        let mut a = account("acc-1");
        a.session_key = Some("sk-ant-secret".to_string());
        let v = build_state(&[a], &BTreeMap::new(), &BTreeMap::new(), None, 0, 120, None);
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("sk-ant-secret"), "state.json must never carry a session key");
        assert!(!text.contains("sessionKey"));
    }

    #[test]
    fn usage_payloads_pass_through_untouched() {
        let mut usage = BTreeMap::new();
        usage.insert(
            "acc-1".to_string(),
            serde_json::json!({"five_hour": {"utilization": 5}, "limits": [{"percent": 38}]}),
        );
        let a = account("acc-1");
        let v = build_state(&[a], &usage, &BTreeMap::new(), None, 0, 120, None);
        assert_eq!(v["usage"]["acc-1"]["five_hour"]["utilization"], 5);
        assert_eq!(v["usage"]["acc-1"]["limits"][0]["percent"], 38);
    }

    #[test]
    fn empty_collections_are_emitted_not_omitted() {
        let v = build_state(&[], &BTreeMap::new(), &BTreeMap::new(), None, 0, 120, None);
        assert!(v["accounts"].is_array());
        assert!(v["usage"].is_object());
        assert!(v["errors"].is_object());
        assert!(v["fatal"].is_null());
        assert!(v["activeClaudeCodeEmail"].is_null());
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let v = build_state(&[], &BTreeMap::new(), &BTreeMap::new(), None, 7, 120, None);
        write_atomic(&path, &v).unwrap();

        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["polledAtMs"], 7);

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != "state.json")
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
    }

    #[test]
    fn a_second_write_replaces_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        write_atomic(&path, &build_state(&[], &BTreeMap::new(), &BTreeMap::new(), None, 1, 120, None)).unwrap();
        write_atomic(&path, &build_state(&[], &BTreeMap::new(), &BTreeMap::new(), None, 2, 120, None)).unwrap();
        let back: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back["polledAtMs"], 2);
    }
}
