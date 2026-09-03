//! Linux browser-profile and Claude cookie discovery.
//!
//! Mirrors `apps/macos/Shared/ChromeCookieService.swift` (the macOS
//! `BrowserCookieService`) for the observable parts that `sync`
//! reproduces, adapted to Linux's Chromium layout (proven in Spike 0):
//!
//! - Profiles live under `$XDG_CONFIG_HOME/{google-chrome, BraveSoftware/
//!   Brave-Browser, microsoft-edge}`, scanned at up to three roots:
//!   `~/.config` (native install), `~/.var/app/<flathub id>/config` (Flatpak
//!   install; the three Flathub manifests keep Flatpak's default config
//!   home and pass no `--user-data-dir`) and, for Brave only,
//!   `~/snap/brave/current/.config` (the official snap; Chrome and Edge
//!   have none). Arc has no Linux build, so it is omitted.
//! - A profile is any immediate subdirectory of that config dir that
//!   contains a `Cookies` file (`Default`, `Profile 1`, ...).
//! - The display name and Google account e-mail come from the browser's
//!   `Local State` JSON at `profile.info_cache[<dir>].name` /
//!   `.user_name` (missing -> the caller falls back to the dir name).
//! - The cookie DB is copied before reading — Chromium holds a WAL lock
//!   while running — and its `meta.version` selects the schema>=24
//!   domain-hash strip in [`crate::cookie::decrypt_cookie_value`].
//! - The `sessionKey` and `lastActiveOrg` cookies for any `%claude.ai%`
//!   host are the two values `sync` needs (session key and org id).
//!
//! Key material (the Safe Storage secret) is *not* read here: the caller
//! fetches it once per browser via [`crate::cookie::keyring_password`] and
//! passes a [`crate::cookie::KeySources`] to the decrypt call.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::model::Browser;

/// One supported Chromium browser's Linux config layout and keyring
/// application name. The same browser can be installed natively, as a
/// Flatpak or (Brave) as a snap; all installs share `subdir` and
/// `keyring_app`, only the config root differs (see [`config_roots`]).
struct BrowserSpec {
    browser: Browser,
    /// Path components of the browser's own config directory, relative to
    /// `$XDG_CONFIG_HOME` (`~/.config` natively,
    /// `~/.var/app/<flatpak_id>/config` inside the Flatpak sandbox,
    /// `~/snap/<snap_name>/current/.config` inside a strict snap, whose
    /// `HOME` is the revision directory).
    subdir: &'static [&'static str],
    /// The Flathub application id. Doubles as the portal `app_id` a
    /// sandboxed install presents (`/.flatpak-info` `[Application] name`).
    flatpak_id: &'static str,
    /// The Snap Store name of the browser's official snap, if one exists.
    /// xdg-desktop-portal presents a snapped caller as `snap.<snap_name>`.
    snap_name: Option<&'static str>,
    /// The `application` attribute used to look up the Safe Storage secret
    /// via `secret-tool` (see [`crate::cookie::keyring_password`]). Identical
    /// for native and Flatpak installs: the Flathub manifests grant
    /// `--talk-name=org.freedesktop.secrets`, and the attribute is a
    /// compile-time constant of the browser.
    keyring_app: &'static str,
}

/// The three Chromium browsers with a Linux build. Arc is macOS/Windows
/// only and is intentionally absent.
const SPECS: &[BrowserSpec] = &[
    BrowserSpec {
        browser: Browser::Chrome,
        subdir: &["google-chrome"],
        flatpak_id: "com.google.Chrome",
        snap_name: None,
        keyring_app: "chrome",
    },
    BrowserSpec {
        browser: Browser::Brave,
        subdir: &["BraveSoftware", "Brave-Browser"],
        flatpak_id: "com.brave.Browser",
        snap_name: Some("brave"),
        keyring_app: "brave",
    },
    BrowserSpec {
        browser: Browser::Edge,
        subdir: &["microsoft-edge"],
        flatpak_id: "com.microsoft.Edge",
        snap_name: None,
        keyring_app: "microsoft-edge",
    },
];

/// The Flathub application id of a browser, or `None` for one with no Linux
/// build (Arc). The single source for that id: profile discovery derives the
/// Flatpak config root from it, and `sync` tries it as a portal `app_id`
/// for `v12` cookies.
pub fn flathub_id(browser: &Browser) -> Option<&'static str> {
    SPECS
        .iter()
        .find(|s| s.browser == *browser)
        .map(|s| s.flatpak_id)
}

/// The Snap Store name of a browser's official snap: `Some("brave")` for
/// Brave, `None` for Chrome and Edge (no official snap) and Arc (no Linux
/// build). The single source for that name: profile discovery derives the
/// snap config root from it, and `sync` tries `snap.<name>` as a portal
/// `app_id` for `v12` cookies.
pub fn snap_name(browser: &Browser) -> Option<&'static str> {
    SPECS
        .iter()
        .find(|s| s.browser == *browser)
        .and_then(|s| s.snap_name)
}

/// The config roots to scan for one browser: the native install's
/// `~/.config/<subdir>` first, then the Flatpak install's
/// `~/.var/app/<flatpak_id>/config/<subdir>` (Flatpak's default
/// `XDG_CONFIG_HOME`; none of the three Flathub manifests overrides it or
/// passes `--user-data-dir`), then, for a browser with an official snap,
/// `~/snap/<snap_name>/current/.config/<subdir>`. snapd sets a strict
/// snap's `HOME` to `~/snap/<name>/<revision>` (`snapenv.go`), the gnome
/// extension's `desktop-launch` then exports
/// `XDG_CONFIG_HOME=$SNAP_USER_DATA/.config` (the same directory), and
/// Brave resolves its data dir from `XDG_CONFIG_HOME` (`chrome_paths_linux.cc`).
/// The snap path deliberately goes through the `current` symlink, which
/// `snap run` creates and repairs on every launch
/// (`createOrUpdateUserDataSymlink`): a stored `cookies_db` then survives
/// the refresh that moves data to a new revision directory.
fn config_roots(home: &Path, spec: &BrowserSpec) -> Vec<PathBuf> {
    let native = home.join(".config");
    let flatpak = home
        .join(".var")
        .join("app")
        .join(spec.flatpak_id)
        .join("config");
    let snap = spec
        .snap_name
        .map(|name| home.join("snap").join(name).join("current").join(".config"));
    [Some(native), Some(flatpak), snap]
        .into_iter()
        .flatten()
        .map(|root| spec.subdir.iter().fold(root, |p, part| p.join(part)))
        .collect()
}

/// A browser profile found on disk that carries a `Cookies` database.
///
/// `profile_dir` is the immediate directory name under the browser's
/// config dir (`"Default"`, `"Profile 1"`, ...) — the value stored as the
/// account's `chromeProfilePath`, matching macOS, where `browser` +
/// `chromeProfilePath` reconstructs the full path.
pub struct DiscoveredProfile {
    pub browser: Browser,
    /// `application` name for the Safe Storage keyring lookup.
    pub keyring_app: String,
    /// Directory name of the profile (the account's `chromeProfilePath`).
    pub profile_dir: String,
    /// `info_cache[<dir>].name` from `Local State`, if present.
    pub display_name: Option<String>,
    /// `info_cache[<dir>].user_name` (the Google account e-mail), if
    /// present. May be an empty string, treated as absent by
    /// [`Self::google_email_nonempty`].
    pub google_email: Option<String>,
    /// Absolute path to the profile's `Cookies` SQLite database.
    pub cookies_db: PathBuf,
}

impl DiscoveredProfile {
    /// The display name, falling back to the profile directory name when
    /// `Local State` carried no `name` for this profile — matching the
    /// Swift `scanProfiles` fallback.
    pub fn resolved_display_name(&self) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| self.profile_dir.clone())
    }

    /// The Google account e-mail, but only when it is present and
    /// non-empty (mirrors the Swift `googleEmail.isEmpty` check).
    pub fn google_email_nonempty(&self) -> Option<String> {
        self.google_email
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
    }
}

/// One row of the Claude cookie query: the cookie `name`, its `host_key`
/// (needed to strip the schema>=24 domain-hash prefix), and the raw
/// `encrypted_value` blob to hand to [`crate::cookie::decrypt_cookie_value`].
pub struct RawCookie {
    pub name: String,
    pub host_key: String,
    pub encrypted_value: Vec<u8>,
}

/// Scans every installed browser's config roots under `home` (native and
/// Flatpak, see `config_roots`) and returns one [`DiscoveredProfile`] per
/// profile directory that contains a `Cookies` file. A root that does not
/// exist contributes nothing.
///
/// Profiles are returned browser-by-browser (Chrome, then Brave, then
/// Edge); within a browser the native install's profiles come before the
/// Flatpak install's, each root's profiles sorted by directory name. A user
/// with both installs therefore sees two profiles that may share a
/// directory name (`Default`): harmless, since accounts are deduplicated by
/// the Claude account uuid, never by profile.
pub fn discover_profiles_under(home: &Path) -> Vec<DiscoveredProfile> {
    let mut out = Vec::new();
    for spec in SPECS {
        for base in config_roots(home, spec) {
            push_profiles_in(&base, spec, &mut out);
        }
    }
    out
}

/// Appends one [`DiscoveredProfile`] per `Cookies`-bearing subdirectory of
/// `base` (one browser install's config dir), sorted by directory name,
/// with display name and e-mail from that root's own `Local State`. A
/// missing or unreadable `base` appends nothing.
fn push_profiles_in(base: &Path, spec: &BrowserSpec, out: &mut Vec<DiscoveredProfile>) {
    if !base.is_dir() {
        return;
    }
    let info_cache = read_info_cache(&base.join("Local State"));

    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    let mut dirs: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| base.join(name).join("Cookies").is_file())
        .collect();
    dirs.sort();

    for dir in dirs {
        let entry = info_cache.as_ref().and_then(|m| m.get(&dir));
        let display_name = entry.and_then(|e| e.name.clone());
        let google_email = entry.and_then(|e| e.user_name.clone());
        out.push(DiscoveredProfile {
            browser: spec.browser.clone(),
            keyring_app: spec.keyring_app.to_string(),
            cookies_db: base.join(&dir).join("Cookies"),
            profile_dir: dir,
            display_name,
            google_email,
        });
    }
}

/// Reads a profile's `Cookies` DB: copies it (and any `-wal`/`-shm`
/// sidecars) to a private temp path to dodge Chromium's WAL lock, opens the
/// copy read-only, and returns `(schema_version, claude_cookie_rows)`.
///
/// `schema_version` is `meta.version` (defaulting to `24` when unreadable —
/// the strip in [`crate::cookie::decrypt_cookie_value`] is itself guarded by
/// a `SHA256(host_key)` prefix match, so assuming the modern schema can only
/// strip a prefix that genuinely matches). Returns `None` only when the copy
/// or the initial open fails. The temp files are removed on return.
pub fn read_claude_cookie_db(cookies_db: &Path) -> Option<(i64, Vec<RawCookie>)> {
    let copy = TempDbCopy::create(cookies_db)?;
    let conn =
        Connection::open_with_flags(copy.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    let schema = read_schema_version(&conn).unwrap_or(24);
    let rows = read_claude_cookies(&conn).unwrap_or_default();
    Some((schema, rows))
}

/// `meta.version` (the Chromium cookie DB schema version), cast to an
/// integer. `None` if the `meta` table or `version` row is absent.
fn read_schema_version(conn: &Connection) -> Option<i64> {
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'version'",
        [],
        |r| r.get(0),
    )
    .ok()
}

/// The `sessionKey` and `lastActiveOrg` cookies for any `%claude.ai%` host.
fn read_claude_cookies(conn: &Connection) -> rusqlite::Result<Vec<RawCookie>> {
    let mut stmt = conn.prepare(
        "SELECT name, host_key, encrypted_value FROM cookies \
         WHERE host_key LIKE '%claude.ai%' AND name IN ('sessionKey', 'lastActiveOrg')",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(RawCookie {
                name: row.get(0)?,
                host_key: row.get(1)?,
                encrypted_value: row.get(2)?,
            })
        })?
        .collect();
    rows
}

/// One `info_cache` entry from `Local State`.
struct InfoCacheEntry {
    name: Option<String>,
    user_name: Option<String>,
}

/// Parses `profile.info_cache` from a browser's `Local State` JSON into a
/// `dir -> {name, user_name}` map. `None` if the file is missing or not the
/// expected shape.
fn read_info_cache(local_state: &Path) -> Option<HashMap<String, InfoCacheEntry>> {
    let data = fs::read_to_string(local_state).ok()?;
    let json: Value = serde_json::from_str(&data).ok()?;
    let cache = json.get("profile")?.get("info_cache")?.as_object()?;
    let mut map = HashMap::new();
    for (dir, info) in cache {
        let name = info.get("name").and_then(Value::as_str).map(String::from);
        let user_name = info
            .get("user_name")
            .and_then(Value::as_str)
            .map(String::from);
        map.insert(dir.clone(), InfoCacheEntry { name, user_name });
    }
    Some(map)
}

/// A per-process-unique counter so concurrent reads never collide on a
/// temp DB name.
static COPY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A copy of a cookie DB (plus any `-wal`/`-shm` sidecars) in the system
/// temp dir, deleted when dropped.
struct TempDbCopy {
    paths: Vec<PathBuf>,
}

impl TempDbCopy {
    fn create(src: &Path) -> Option<Self> {
        let mut main = std::env::temp_dir();
        let n = COPY_COUNTER.fetch_add(1, Ordering::Relaxed);
        main.push(format!(
            "claude-dashboard-cookies-{}-{}.db",
            std::process::id(),
            n
        ));
        let _ = fs::remove_file(&main);
        fs::copy(src, &main).ok()?;
        let mut paths = vec![main.clone()];
        for suffix in ["-wal", "-shm"] {
            let side = with_suffix(src, suffix);
            if side.is_file() {
                let dst = with_suffix(&main, suffix);
                if fs::copy(&side, &dst).is_ok() {
                    paths.push(dst);
                }
            }
        }
        Some(TempDbCopy { paths })
    }

    fn path(&self) -> &Path {
        &self.paths[0]
    }
}

impl Drop for TempDbCopy {
    fn drop(&mut self) {
        for p in &self.paths {
            let _ = fs::remove_file(p);
        }
    }
}

/// Appends `suffix` to a path's full file name (SQLite's `-wal`/`-shm`
/// sidecars are `<dbfile>-wal`, not a new extension).
fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn finds_default_profile_and_reads_display_name_and_email() {
        let d = tempfile::tempdir().unwrap();
        let cfg = d.path().join(".config/google-chrome");
        write(
            &cfg.join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"Work","user_name":"me@x.com"}}}}"#,
        );
        write(&cfg.join("Default/Cookies"), "");

        let profiles = discover_profiles_under(d.path());
        let p = profiles
            .iter()
            .find(|p| matches!(p.browser, Browser::Chrome) && p.profile_dir == "Default")
            .expect("Chrome Default profile discovered");
        assert_eq!(p.display_name.as_deref(), Some("Work"));
        assert_eq!(p.google_email.as_deref(), Some("me@x.com"));
        assert_eq!(p.keyring_app, "chrome");
        assert!(p.cookies_db.ends_with("google-chrome/Default/Cookies"));
    }

    #[test]
    fn discovers_brave_and_edge_profiles() {
        let d = tempfile::tempdir().unwrap();
        let brave = d.path().join(".config/BraveSoftware/Brave-Browser");
        write(
            &brave.join("Local State"),
            r#"{"profile":{"info_cache":{"Profile 1":{"name":"Brave Me"}}}}"#,
        );
        write(&brave.join("Profile 1/Cookies"), "");
        let edge = d.path().join(".config/microsoft-edge");
        write(&edge.join("Default/Cookies"), ""); // no Local State -> dir-name fallback

        let profiles = discover_profiles_under(d.path());

        let b = profiles
            .iter()
            .find(|p| matches!(p.browser, Browser::Brave))
            .expect("Brave profile discovered");
        assert_eq!(b.profile_dir, "Profile 1");
        assert_eq!(b.resolved_display_name(), "Brave Me");
        assert_eq!(b.keyring_app, "brave");

        let e = profiles
            .iter()
            .find(|p| matches!(p.browser, Browser::Edge))
            .expect("Edge profile discovered");
        assert_eq!(e.display_name, None);
        assert_eq!(e.resolved_display_name(), "Default"); // falls back to dir name
        assert_eq!(e.keyring_app, "microsoft-edge");
    }

    #[test]
    fn subdir_without_cookies_file_is_not_a_profile() {
        let d = tempfile::tempdir().unwrap();
        let cfg = d.path().join(".config/google-chrome");
        fs::create_dir_all(cfg.join("Profile 9")).unwrap(); // no Cookies file
        write(&cfg.join("Local State"), r#"{"profile":{"info_cache":{}}}"#);

        assert!(discover_profiles_under(d.path()).is_empty());
    }

    #[test]
    fn missing_config_dirs_yield_no_profiles() {
        let d = tempfile::tempdir().unwrap();
        assert!(discover_profiles_under(d.path()).is_empty());
    }

    #[test]
    fn google_email_nonempty_treats_blank_as_absent() {
        let p = DiscoveredProfile {
            browser: Browser::Chrome,
            keyring_app: "chrome".into(),
            profile_dir: "Default".into(),
            display_name: Some("Work".into()),
            google_email: Some(String::new()),
            cookies_db: PathBuf::from("/x/Cookies"),
        };
        assert_eq!(p.google_email_nonempty(), None);
    }

    #[test]
    fn read_claude_cookie_db_reads_schema_and_filters_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("Cookies");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta(key TEXT NOT NULL UNIQUE, value TEXT);
                 INSERT INTO meta VALUES('version','24');
                 CREATE TABLE cookies(name TEXT, host_key TEXT, encrypted_value BLOB);
                 INSERT INTO cookies VALUES('sessionKey','.claude.ai', x'763130AABB');
                 INSERT INTO cookies VALUES('lastActiveOrg','claude.ai', x'763130CCDD');
                 INSERT INTO cookies VALUES('cf_bm','claude.ai', x'00');
                 INSERT INTO cookies VALUES('sessionKey','example.com', x'01');",
            )
            .unwrap();
        }
        let (schema, rows) = read_claude_cookie_db(&db).expect("db read");
        assert_eq!(schema, 24);
        assert_eq!(rows.len(), 2); // only claude.ai sessionKey + lastActiveOrg
        assert!(rows.iter().any(|r| r.name == "sessionKey" && r.host_key == ".claude.ai"));
        assert!(rows.iter().any(|r| r.name == "lastActiveOrg" && r.host_key == "claude.ai"));
        // encrypted_value blobs come through as raw bytes.
        let sk = rows.iter().find(|r| r.name == "sessionKey").unwrap();
        assert_eq!(sk.encrypted_value, vec![0x76, 0x31, 0x30, 0xAA, 0xBB]);
    }

    #[test]
    fn read_claude_cookie_db_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_claude_cookie_db(&dir.path().join("nope/Cookies")).is_none());
    }

    #[test]
    fn discovers_a_flatpak_brave_profile_under_var_app() {
        let d = tempfile::tempdir().unwrap();
        let cfg = d
            .path()
            .join(".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser");
        write(
            &cfg.join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"Flat Brave","user_name":"flat@x.com"}}}}"#,
        );
        write(&cfg.join("Default/Cookies"), "");

        let profiles = discover_profiles_under(d.path());
        assert_eq!(profiles.len(), 1);
        let p = &profiles[0];
        assert!(matches!(p.browser, Browser::Brave));
        // Same host keyring item as the native install: the Flathub manifest
        // grants --talk-name=org.freedesktop.secrets and the `application`
        // attribute is a compile-time constant of the browser.
        assert_eq!(p.keyring_app, "brave");
        assert_eq!(p.profile_dir, "Default");
        // Local State is read from the Flatpak root, not ~/.config.
        assert_eq!(p.display_name.as_deref(), Some("Flat Brave"));
        assert_eq!(p.google_email.as_deref(), Some("flat@x.com"));
        assert!(p.cookies_db.ends_with(
            ".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/Default/Cookies"
        ));
    }

    #[test]
    fn native_and_flatpak_installs_of_one_browser_are_both_discovered_grouped_by_browser() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join(".config/google-chrome/Default/Cookies"), "");
        write(
            &d.path()
                .join(".var/app/com.google.Chrome/config/google-chrome/Default/Cookies"),
            "",
        );
        write(
            &d.path()
                .join(".config/BraveSoftware/Brave-Browser/Default/Cookies"),
            "",
        );

        let profiles = discover_profiles_under(d.path());
        let browsers: Vec<&Browser> = profiles.iter().map(|p| &p.browser).collect();
        assert_eq!(
            browsers,
            vec![&Browser::Chrome, &Browser::Chrome, &Browser::Brave]
        );
        assert!(profiles[0].cookies_db.starts_with(d.path().join(".config")));
        assert!(profiles[1]
            .cookies_db
            .starts_with(d.path().join(".var/app")));
    }

    #[test]
    fn flathub_id_is_known_for_each_linux_browser_and_absent_for_arc() {
        assert_eq!(flathub_id(&Browser::Chrome), Some("com.google.Chrome"));
        assert_eq!(flathub_id(&Browser::Brave), Some("com.brave.Browser"));
        assert_eq!(flathub_id(&Browser::Edge), Some("com.microsoft.Edge"));
        assert_eq!(flathub_id(&Browser::Arc), None);
    }

    #[test]
    fn discovers_a_snap_brave_profile_through_the_current_symlink() {
        let d = tempfile::tempdir().unwrap();
        // snapd layout: HOME=~/snap/brave/<revision>, `current` -> <revision>.
        let rev = d.path().join("snap/brave/x676");
        let cfg = rev.join(".config/BraveSoftware/Brave-Browser");
        write(
            &cfg.join("Local State"),
            r#"{"profile":{"info_cache":{"Default":{"name":"Snap Brave","user_name":"snap@x.com"}}}}"#,
        );
        write(&cfg.join("Default/Cookies"), "");
        std::os::unix::fs::symlink(&rev, d.path().join("snap/brave/current")).unwrap();

        let profiles = discover_profiles_under(d.path());
        assert_eq!(profiles.len(), 1);
        let p = &profiles[0];
        assert!(matches!(p.browser, Browser::Brave));
        assert_eq!(p.keyring_app, "brave");
        assert_eq!(p.profile_dir, "Default");
        assert_eq!(p.display_name.as_deref(), Some("Snap Brave"));
        assert_eq!(p.google_email.as_deref(), Some("snap@x.com"));
        // The stored path goes through `current`, so it survives a snap
        // refresh that moves the data to a new revision directory.
        assert!(p
            .cookies_db
            .ends_with("snap/brave/current/.config/BraveSoftware/Brave-Browser/Default/Cookies"));
    }

    #[test]
    fn native_flatpak_and_snap_installs_of_brave_are_returned_in_that_order() {
        let d = tempfile::tempdir().unwrap();
        write(
            &d.path()
                .join("snap/brave/current/.config/BraveSoftware/Brave-Browser/Default/Cookies"),
            "",
        );
        write(
            &d.path().join(
                ".var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/Default/Cookies",
            ),
            "",
        );
        write(
            &d.path()
                .join(".config/BraveSoftware/Brave-Browser/Default/Cookies"),
            "",
        );

        let profiles = discover_profiles_under(d.path());
        let roots: Vec<PathBuf> = profiles
            .iter()
            .map(|p| {
                p.cookies_db
                    .strip_prefix(d.path())
                    .unwrap()
                    .iter()
                    .take(1)
                    .collect()
            })
            .collect();
        assert_eq!(
            roots,
            vec![
                PathBuf::from(".config"),
                PathBuf::from(".var"),
                PathBuf::from("snap")
            ]
        );
    }

    #[test]
    fn only_brave_has_an_official_snap() {
        assert_eq!(snap_name(&Browser::Brave), Some("brave"));
        assert_eq!(snap_name(&Browser::Chrome), None);
        assert_eq!(snap_name(&Browser::Edge), None);
        assert_eq!(snap_name(&Browser::Arc), None);
    }
}
