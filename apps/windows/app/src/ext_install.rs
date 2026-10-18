//! The Add Account wizard's per-profile extension install: which profiles have
//! the extension, and what Install opens. No browser installs an extension
//! silently, so Install opens the store page in that exact profile; until the
//! extension is published it falls back to the unpacked copy embedded here.
//! See docs/superpowers/specs/2026-10-17-windows-extension-install-design.md.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use claude_dashboard_core::browser::WindowsProfile;
use claude_dashboard_core::model::Browser;
use serde_json::Value;

/// The unpacked build's id, fixed by the manifest `key`.
pub const UNPACKED_ID: &str = "cadpjcfajhlgdaipepkdojcehfmkkedh";
/// The Chrome Web Store id, once published. Must also be in both host manifests'
/// `allowed_origins` (a test checks).
pub const CHROME_WEB_STORE_ID: Option<&str> = None;
/// The Edge Add-ons id, once published (same rule).
pub const EDGE_ADDONS_ID: Option<&str> = None;

/// Every id the extension can have in a profile.
pub fn known_ids() -> Vec<&'static str> {
    [Some(UNPACKED_ID), CHROME_WEB_STORE_ID, EDGE_ADDONS_ID].into_iter().flatten().collect()
}

/// The extension in one profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtState {
    NotInstalled,
    Installed,
    TurnedOff,
}

impl ExtState {
    /// The `ExtProfile.state` value the wizard UI switches on.
    pub fn ui(self) -> i32 {
        match self {
            ExtState::NotInstalled => 0,
            ExtState::Installed => 1,
            ExtState::TurnedOff => 2,
        }
    }
}

/// One `extensions.settings.<id>` entry. Chrome keeps id-only tombstones too, so
/// an entry counts only when it has a `location`. Off: a non-empty
/// `disable_reasons` (a list today, a bitmask before) or the legacy `state: 0`.
fn entry_state(entry: &Value) -> ExtState {
    if entry.get("location").is_none() {
        return ExtState::NotInstalled;
    }
    let off = match entry.get("disable_reasons") {
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Number(n)) => n.as_u64() != Some(0),
        _ => false,
    } || entry.get("state").and_then(Value::as_u64) == Some(0);
    if off {
        ExtState::TurnedOff
    } else {
        ExtState::Installed
    }
}

/// The best state of `ids` in one preferences document: installed beats off.
pub fn state_from_prefs(prefs: &Value, ids: &[&str]) -> ExtState {
    let Some(settings) = prefs.pointer("/extensions/settings") else {
        return ExtState::NotInstalled;
    };
    let mut best = ExtState::NotInstalled;
    for id in ids {
        match settings.get(*id).map(entry_state) {
            Some(ExtState::Installed) => return ExtState::Installed,
            Some(ExtState::TurnedOff) => best = ExtState::TurnedOff,
            _ => {}
        }
    }
    best
}

/// A profile's own directory (`...\User Data\<profile_dir>`).
pub fn profile_path(p: &WindowsProfile) -> PathBuf {
    p.local_state.parent().map(|d| d.join(&p.profile_dir)).unwrap_or_default()
}

/// Reads profile states, re-parsing a preferences file only when its mtime or
/// size changed (the wizard polls every 2 s and `Secure Preferences` can be large).
#[derive(Default)]
pub struct StateCache {
    seen: HashMap<PathBuf, (SystemTime, u64, ExtState)>,
}

impl StateCache {
    /// `Secure Preferences` then `Preferences` (Chrome keeps extensions in the
    /// first; a browser without protected prefs uses the second).
    pub fn profile_state(&mut self, profile: &Path, ids: &[&str]) -> ExtState {
        let mut best = ExtState::NotInstalled;
        for name in ["Secure Preferences", "Preferences"] {
            match self.file_state(&profile.join(name), ids) {
                ExtState::Installed => return ExtState::Installed,
                ExtState::TurnedOff => best = ExtState::TurnedOff,
                ExtState::NotInstalled => {}
            }
        }
        best
    }

    fn file_state(&mut self, path: &Path, ids: &[&str]) -> ExtState {
        let Ok(meta) = fs::metadata(path) else {
            return ExtState::NotInstalled;
        };
        let (mtime, len) = (meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len());
        if let Some((m, l, s)) = self.seen.get(path) {
            if *m == mtime && *l == len {
                return *s;
            }
        }
        let state = fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| state_from_prefs(&v, ids))
            .unwrap_or(ExtState::NotInstalled);
        self.seen.insert(path.to_path_buf(), (mtime, len, state));
        state
    }
}

/// One row of the extension tab.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtRow {
    pub browser: Browser,
    pub profile_dir: String,
    pub label: String,
    pub detail: String,
    pub state: ExtState,
}

/// The extension tab's rows: every discovered profile, in discovery order.
pub fn rows(profiles: &[WindowsProfile], cache: &mut StateCache) -> Vec<ExtRow> {
    let ids = known_ids();
    profiles
        .iter()
        .map(|p| ExtRow {
            browser: p.browser.clone(),
            profile_dir: p.profile_dir.clone(),
            label: format!(
                "{} \u{00B7} {}",
                p.browser.display_name(),
                p.display_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| p.profile_dir.clone())
            ),
            detail: p.google_email.clone().unwrap_or_default(),
            state: cache.profile_state(&profile_path(p), &ids),
        })
        .collect()
}

/// The rows for every browser under `%LOCALAPPDATA%`; empty when it is unset.
pub fn current_rows(cache: &mut StateCache) -> Vec<ExtRow> {
    let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) else {
        return Vec::new();
    };
    let profiles = claude_dashboard_core::browser::discover_windows_profiles_under(Path::new(&local));
    rows(&profiles, cache)
}

/// What a row's button does in its profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Open this URL.
    Open(String),
    /// Browsers ignore `chrome://` URLs passed on the command line, so the
    /// extensions page can't be opened for the user: open the local setup page
    /// that says how to reach it. `unpack`: write the unpacked copy first.
    Guide { unpack: bool },
}

pub fn action(row: &ExtRow) -> Action {
    match row.state {
        ExtState::Installed => Action::Open("https://claude.ai".to_string()),
        ExtState::TurnedOff => Action::Guide { unpack: false },
        ExtState::NotInstalled => match store_url(&row.browser) {
            Some(url) => Action::Open(url),
            None => Action::Guide { unpack: true },
        },
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The setup page for `browser`: how to open its extensions page, turn the
/// extension on, and (with `folder`) load the unpacked copy from that folder.
pub fn guide_html(browser: &Browser, folder: Option<&Path>) -> String {
    let page = extensions_page(browser);
    let load = folder
        .map(|f| {
            let f = html_escape(&f.display().to_string());
            format!(
                "<li>Not listed? Turn on <b>Developer mode</b> (top right), click <b>Load unpacked</b> \
                 and paste this folder: <span class=c>{f}</span> <button data-copy=\"{f}\">Copy</button></li>"
            )
        })
        .unwrap_or_default();
    format!(
        "<!doctype html><meta charset=utf-8><title>Install Claude Dashboard Sync</title>\
         <style>body{{font:15px 'Segoe UI',sans-serif;margin:3rem auto;max-width:36rem;line-height:1.6;padding:0 1rem}}\
         .c{{font-family:Consolas,monospace;background:#8882;padding:2px 6px;border-radius:4px;word-break:break-all}}\
         button{{font:inherit;margin-left:6px}}li{{margin:.6rem 0}}</style>\
         <h2>Install Claude Dashboard Sync</h2>\
         <p>{name} does not let other apps open its extensions page, so:</p><ol>\
         <li>Copy <span class=c>{page}</span> <button data-copy=\"{page}\">Copy</button>, \
         paste it into the address bar above and press Enter.</li>\
         <li>Listed but off? Switch <b>Claude Dashboard Sync</b> on.</li>{load}\
         <li>Open <a href=\"https://claude.ai\">claude.ai</a> signed in: the extension sends the key \
         and the account appears in Claude Dashboard.</li></ol>\
         <script>for(const b of document.querySelectorAll('button'))b.onclick=()=>\
         navigator.clipboard.writeText(b.dataset.copy).then(()=>b.textContent='Copied');</script>",
        name = html_escape(browser.display_name()),
    )
}

/// `%LOCALAPPDATA%\claude-dashboard\extension-setup.html`, beside the unpacked folder.
pub fn guide_path() -> Option<PathBuf> {
    unpacked_dir().map(|d| d.with_file_name("extension-setup.html"))
}

/// A `file:///` URL for a local Windows path.
pub fn file_url(path: &Path) -> String {
    let mut out = String::from("file:///");
    for c in path.display().to_string().chars() {
        match c {
            '\\' => out.push('/'),
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            '?' => out.push_str("%3F"),
            c => out.push(c),
        }
    }
    out
}

/// What Install opens for `browser`: its store page when published, else `None`
/// (the unpacked fallback).
pub fn store_url(browser: &Browser) -> Option<String> {
    if *browser == Browser::Edge {
        if let Some(id) = EDGE_ADDONS_ID {
            return Some(format!("https://microsoftedge.microsoft.com/addons/detail/{id}"));
        }
    }
    CHROME_WEB_STORE_ID.map(|id| format!("https://chromewebstore.google.com/detail/{id}"))
}

/// The browser's own extensions page (Turn on, and Load unpacked).
pub fn extensions_page(browser: &Browser) -> &'static str {
    match browser {
        Browser::Edge => "edge://extensions",
        Browser::Brave => "brave://extensions",
        _ => "chrome://extensions",
    }
}

/// The extension files, embedded so the unpacked fallback needs no download.
/// Same list as `scripts/pack-extension.ps1` (a test checks).
const EMBEDDED: &[(&str, &[u8])] = &[
    ("manifest.json", include_bytes!("../../extension/manifest.json")),
    ("background.js", include_bytes!("../../extension/background.js")),
    ("popup.html", include_bytes!("../../extension/popup.html")),
    ("popup.js", include_bytes!("../../extension/popup.js")),
    ("lib/extension-id.js", include_bytes!("../../extension/lib/extension-id.js")),
    ("lib/status.js", include_bytes!("../../extension/lib/status.js")),
    ("lib/sync.js", include_bytes!("../../extension/lib/sync.js")),
    ("icons/icon-16.png", include_bytes!("../../extension/icons/icon-16.png")),
    ("icons/icon-32.png", include_bytes!("../../extension/icons/icon-32.png")),
    ("icons/icon-48.png", include_bytes!("../../extension/icons/icon-48.png")),
    ("icons/icon-128.png", include_bytes!("../../extension/icons/icon-128.png")),
];

/// `%LOCALAPPDATA%\claude-dashboard\extension`, the folder `install.ps1` unpacks to.
pub fn unpacked_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(local).join("claude-dashboard").join("extension"))
}

/// Writes the embedded extension into `dir`, leaving files that already match
/// (a loaded unpacked extension keeps reading the same files).
pub fn write_unpacked(dir: &Path) -> std::io::Result<()> {
    for (name, bytes) in EMBEDDED {
        let path = dir.join(name);
        if fs::read(&path).map(|b| b == *bytes).unwrap_or(false) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn prefs(id: &str, entry: Value) -> Value {
        json!({ "extensions": { "settings": { id: entry } } })
    }

    #[test]
    fn an_enabled_entry_is_installed() {
        let p = prefs(UNPACKED_ID, json!({ "location": 4, "path": "C:\\x" }));
        assert_eq!(state_from_prefs(&p, &[UNPACKED_ID]), ExtState::Installed);
        let empty = prefs(UNPACKED_ID, json!({ "location": 1, "disable_reasons": [] }));
        assert_eq!(state_from_prefs(&empty, &[UNPACKED_ID]), ExtState::Installed);
    }

    #[test]
    fn disable_reasons_or_legacy_state_mean_turned_off() {
        for entry in [
            json!({ "location": 1, "disable_reasons": [1] }),
            json!({ "location": 1, "disable_reasons": 8 }),
            json!({ "location": 4, "state": 0 }),
        ] {
            assert_eq!(state_from_prefs(&prefs("x", entry), &["x"]), ExtState::TurnedOff);
        }
    }

    #[test]
    fn tombstones_other_ids_and_missing_settings_are_not_installed() {
        let tomb = prefs(UNPACKED_ID, json!({ "disable_reasons": [1] }));
        assert_eq!(state_from_prefs(&tomb, &[UNPACKED_ID]), ExtState::NotInstalled);
        let other = prefs("abc", json!({ "location": 1 }));
        assert_eq!(state_from_prefs(&other, &[UNPACKED_ID]), ExtState::NotInstalled);
        assert_eq!(state_from_prefs(&json!({}), &[UNPACKED_ID]), ExtState::NotInstalled);
    }

    #[test]
    fn installed_under_any_id_beats_off_under_another() {
        let p = json!({ "extensions": { "settings": {
            "a": { "location": 4, "disable_reasons": [1] },
            "b": { "location": 1 }
        } } });
        assert_eq!(state_from_prefs(&p, &["a", "b"]), ExtState::Installed);
        assert_eq!(state_from_prefs(&p, &["a"]), ExtState::TurnedOff);
    }

    #[test]
    fn the_cache_reads_both_files_and_sees_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = StateCache::default();
        assert_eq!(cache.profile_state(dir.path(), &["x"]), ExtState::NotInstalled);
        fs::write(dir.path().join("Preferences"), prefs("x", json!({ "location": 1 })).to_string()).unwrap();
        assert_eq!(cache.profile_state(dir.path(), &["x"]), ExtState::Installed);
        // Secure Preferences wins when it says installed; off there falls back to Preferences.
        fs::write(
            dir.path().join("Secure Preferences"),
            prefs("x", json!({ "location": 1, "disable_reasons": [1] })).to_string(),
        )
        .unwrap();
        assert_eq!(cache.profile_state(dir.path(), &["x"]), ExtState::Installed);
        fs::remove_file(dir.path().join("Preferences")).unwrap();
        assert_eq!(cache.profile_state(dir.path(), &["x"]), ExtState::TurnedOff);
    }

    fn profile(root: &Path, browser: Browser, dir: &str, name: Option<&str>, email: Option<&str>) -> WindowsProfile {
        WindowsProfile {
            browser,
            profile_dir: dir.into(),
            display_name: name.map(Into::into),
            google_email: email.map(Into::into),
            cookies_db: root.join(dir).join("Network").join("Cookies"),
            local_state: root.join("Local State"),
        }
    }

    #[test]
    fn rows_label_each_profile_and_read_its_own_folder() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("Profile 1")).unwrap();
        fs::write(
            root.path().join("Profile 1").join("Secure Preferences"),
            prefs(UNPACKED_ID, json!({ "location": 4 })).to_string(),
        )
        .unwrap();
        let profiles = [
            profile(root.path(), Browser::Chrome, "Default", None, None),
            profile(root.path(), Browser::Chrome, "Profile 1", Some("Work"), Some("w@x.io")),
        ];
        let got = rows(&profiles, &mut StateCache::default());
        assert_eq!(got[0].label, "Google Chrome \u{00B7} Default");
        assert_eq!((got[0].detail.as_str(), got[0].state), ("", ExtState::NotInstalled));
        assert_eq!(got[1].label, "Google Chrome \u{00B7} Work");
        assert_eq!((got[1].detail.as_str(), got[1].state), ("w@x.io", ExtState::Installed));
    }

    #[test]
    fn each_state_opens_its_page() {
        let row = |state| ExtRow {
            browser: Browser::Edge,
            profile_dir: "Default".into(),
            label: String::new(),
            detail: String::new(),
            state,
        };
        assert_eq!(action(&row(ExtState::Installed)), Action::Open("https://claude.ai".into()));
        assert_eq!(action(&row(ExtState::TurnedOff)), Action::Guide { unpack: false });
        let install = action(&row(ExtState::NotInstalled));
        match store_url(&Browser::Edge) {
            Some(url) => assert_eq!(install, Action::Open(url)),
            None => assert_eq!(install, Action::Guide { unpack: true }),
        }
    }

    #[test]
    fn the_guide_names_the_browser_page_and_escapes_the_folder() {
        let html = guide_html(&Browser::Edge, Some(Path::new(r"C:\Users\A&B\x")));
        assert!(html.contains("data-copy=\"edge://extensions\""));
        assert!(html.contains("Microsoft Edge does not let"));
        assert!(html.contains(r"C:\Users\A&amp;B\x"));
        assert!(!html.contains(r"A&B"));
        let off_only = guide_html(&Browser::Chrome, None);
        assert!(off_only.contains("chrome://extensions"));
        assert!(!off_only.contains("Load unpacked"));
    }

    #[test]
    fn file_urls_use_slashes_and_escape_spaces() {
        assert_eq!(
            file_url(Path::new(r"C:\Users\Jo Doe\AppData\Local\claude-dashboard\extension-setup.html")),
            "file:///C:/Users/Jo%20Doe/AppData/Local/claude-dashboard/extension-setup.html"
        );
    }

    #[test]
    fn unpublished_store_ids_fall_back_to_unpacked() {
        // Flip these expectations when the store ids are filled in.
        assert_eq!(store_url(&Browser::Chrome), CHROME_WEB_STORE_ID.map(|id| format!("https://chromewebstore.google.com/detail/{id}")));
        assert_eq!(known_ids()[0], UNPACKED_ID);
        assert_eq!(extensions_page(&Browser::Edge), "edge://extensions");
        assert_eq!(extensions_page(&Browser::ChromeDev), "chrome://extensions");
        assert_eq!(extensions_page(&Browser::Brave), "brave://extensions");
    }

    #[test]
    fn every_known_id_is_allowed_by_both_host_manifests() {
        for manifest in [
            include_str!("../../bridge/com.claude_dashboard.bridge.json"),
            include_str!("../wix/com.claude_dashboard.bridge.json"),
        ] {
            let v: Value = serde_json::from_str(manifest).unwrap();
            let origins: Vec<&str> =
                v["allowed_origins"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
            for id in known_ids() {
                let origin = format!("chrome-extension://{id}/");
                assert!(origins.contains(&origin.as_str()), "{origin} missing from a host manifest");
            }
        }
    }

    #[test]
    fn the_embedded_files_match_the_pack_script_allowlist() {
        let script = include_str!("../../scripts/pack-extension.ps1");
        let start = script.find("$allow = @(").unwrap();
        let end = start + script[start..].find(')').unwrap();
        let listed: Vec<&str> = script[start..end].split('\'').skip(1).step_by(2).collect();
        let embedded: Vec<&str> = EMBEDDED.iter().map(|(n, _)| *n).collect();
        assert_eq!(listed, embedded);
    }

    #[test]
    fn write_unpacked_creates_and_refreshes_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        write_unpacked(dir.path()).unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(dir.path().join("manifest.json")).unwrap()).unwrap();
        assert!(manifest["key"].is_string());
        fs::write(dir.path().join("lib/sync.js"), b"stale").unwrap();
        write_unpacked(dir.path()).unwrap();
        assert_ne!(fs::read(dir.path().join("lib/sync.js")).unwrap(), b"stale");
    }
}
