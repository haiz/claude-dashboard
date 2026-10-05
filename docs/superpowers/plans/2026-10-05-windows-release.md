# Windows Sub-project 6: Release — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Windows app. This sub-project delivers:
- a per-user MSI (no admin), built by `cargo-wix`, that installs the app and the bridge, registers the native-messaging host and adds a Start Menu shortcut
- a zip of the browser extension
- a `release-windows.yml` workflow that uploads both artifacts to the GitHub release `release.sh` creates
- in-app auto-update: GitHub `releases/latest` → download the MSI → `msiexec /passive` → relaunch
- version sync covering the Windows files

**Architecture:**
- Decision logic is pure and TDD'd. Version compare, release-payload parsing and the "is it due" check go in `core::update`. Install-dir detection, the relaunch script, the MSI magic check and the Updates status text go in the app's `updater.rs`.
- The installer is a hand-written `wix/main.wxs` for the app package, built by a PowerShell script (`apps/windows/scripts/build-msi.ps1`). CI and local builds share that script.
- `release-windows.yml` copies `release-linux.yml`'s five guardrails. `scripts/test-release-workflow.sh` is generalised to enforce them on both workflows.

**Tech Stack:** Rust 1.98.0 (pinned), `ureq` 2 (already a `core` dep), WiX Toolset **3.14.1** (binaries zip, no admin), `cargo-wix` **0.3.9**, PowerShell 5.1, bash (Git Bash on Windows), GitHub Actions `windows-latest`.

**Spec:** `docs/superpowers/specs/2026-10-04-windows-app-design.md`, sections "Release" (lines 248–259) and "Registration" (lines 159–165). Out of scope per the spec: code signing, winget/Scoop, publishing to the Store or Add-ons, and `v20` decryption. Resume context: `docs/superpowers/HANDOFF-windows-port.md`.

**Port sources:** `apps/macos/ClaudeDashboard/Services/UpdateService.swift` (check/download/install), `apps/macos/ClaudeDashboard/ViewModels/UpdateViewModel.swift` (auto-update default on, every 24 h, hourly timer, `DISABLE_AUTOUPDATER=1`), `apps/linux/lib/update.js` (pure `isNewer` / `versionFromTag` / `releaseIfNewer` / `shouldCheck`), `.github/workflows/release-linux.yml` and `scripts/test-release-workflow.sh` (guardrails), `scripts/sync-version.sh` / `scripts/test-sync-version.sh` / `scripts/release.sh`, and `apps/windows/scripts/register-dev-host.ps1` (manifest shape).

## Global Constraints

- **Toolchain:** 1.98.0 pinned; `rust-version = 1.89`. `core` must build and test on Linux too: `cd apps/linux && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` runs in CI. On this Windows machine, run `-p claude-dashboard-core` instead, because the helper's XDG tests are Unix-only.
- **Checks on every task:** in `apps/windows`, `cargo test --workspace` and `cargo clippy --all-targets -- -D warnings` must pass.
- **Artifact names (exact):**
  - `ClaudeDashboard-x64.msi`
  - `claude-dashboard-extension.zip`
  - The workflow must never name `ClaudeDashboard.app.zip` or `claude-dashboard-cli.tar.gz`; those belong to `release.sh`.
- **Install location:** `%LOCALAPPDATA%\Programs\ClaudeDashboard\`, containing `claude-dashboard.exe`, `claude-dashboard-bridge.exe` and `com.claude_dashboard.bridge.json`. The install is per user (`InstallScope="perUser"`, `InstallPrivileges="limited"`), x64, with a fixed `UpgradeCode` **`173B2BBC-0CBB-472C-B3BF-E88EE21C4D58`**.
- **Native messaging:**
  - Host name: `com.claude_dashboard.bridge`.
  - Registry keys (default value = absolute manifest path): `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.claude_dashboard.bridge` and `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.claude_dashboard.bridge`.
  - The shipped manifest uses a **relative** `"path": "claude-dashboard-bridge.exe"`; Chromium resolves it against the manifest's directory on Windows.
  - `allowed_origins`: `["chrome-extension://cadpjcfajhlgdaipepkdojcehfmkkedh/"]`.
- **Installer and user data:** the installer never touches `%APPDATA%\claude-dashboard` or `%LOCALAPPDATA%\claude-dashboard`. User data survives both upgrade and uninstall.
- **Update source:** `https://api.github.com/repos/haiz/claude-dashboard/releases/latest`, with headers `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28` and `User-Agent: claude-dashboard/<version>`. Tags are `vX.Y.Z`. The auto-check runs at most every **86400 s**, re-evaluated hourly. Auto-update is **on by default**. `DISABLE_AUTOUPDATER=1` turns it off.
- **WiX:**
  - Pinned to `https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip`, SHA-256 `6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31`, extracted so that `candle.exe` sits at `<WIX>\bin\candle.exe`.
  - On this machine it is already installed at `%LOCALAPPDATA%\Programs\wix314\` (`bin\candle.exe`), and `cargo-wix 0.3.9` is installed.
  - Set `$env:WIX = "$env:LOCALAPPDATA\Programs\wix314\"` before `cargo wix`.
- **Version:** one source, `/VERSION` (`X.Y.Z`). `sync-version.sh` writes it into `apps/windows/Cargo.toml` (`[workspace.package] version`), `apps/windows/Cargo.lock` (members `claude-dashboard`, `claude-dashboard-bridge`, `claude-dashboard-core`) and `apps/windows/extension/manifest.json` (`"version"`).
- **Logs:** the session key never appears in any log, script or error. A downloaded file path may appear in logs; nothing secret goes in the relaunch script.
- **Editing and commits:**
  - Edit `.rs`, `.slint`, `.md`, `.sh`, `.ps1`, `.wxs`, `.yml` and `.json` with the Edit/Write tools. Never use perl, sed or `Set-Content`. `.ps1` files must be saved without a BOM, as UTF-8 ASCII-safe text.
  - Commit with `git -c user.email="backend@gotitapp.co" -c user.name="cthai" commit …` and the trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Rulings (decided here)

1. **`sync-version.sh` becomes portable.** Today it uses `sed -i ''`, which is BSD-only and breaks on GNU sed (Linux CI, Git Bash). Every in-place edit goes through one helper, `sedi() { sed -i.bak "$@" && rm -f "${@: -1}.bak"; }`, which works on both. That lets `test-sync-version.sh` run on this machine and in CI. *Cost if wrong:* none on macOS, where `-i.bak` is valid BSD sed.
2. **The MSI ships a static, relative-path host manifest** instead of templating an absolute path at install time. Chromium on Windows accepts a path relative to the manifest file. `register-dev-host.ps1` keeps writing absolute paths for dev builds. *Cost if wrong:* the extension can't reach the bridge after install. The controller install check (Task 5) catches this before release.
3. **Auto-update acts only on an installed copy.** It must be running from `%LOCALAPPDATA%\Programs\ClaudeDashboard\claude-dashboard.exe`, compared case-insensitively. A dev build (`target\debug`) can still check, but shows "Development build — install the MSI to update" and never runs `msiexec`.
4. **Auto-update installs on its own,** as on macOS: when the daily check finds a newer release, it downloads, runs the relauncher and quits. A manual "Check for Updates" shows the result and an **Install** button.
5. **The relauncher is a hidden, detached PowerShell process.** It waits for the app's PID to exit (up to 30 s), then runs `msiexec /i <msi> /passive /norestart` and waits for it, then starts the installed exe. Waiting first means Windows Installer never meets a locked exe, and the single-instance mutex is free by the time the app relaunches. Every path is single-quoted, with `'` doubled.
6. **A download must be a real MSI before it runs.** A file that doesn't start with the OLE compound-file magic `D0 CF 11 E0 A1 B1 1A E1`, or is larger than 200 MiB, is rejected and deleted.
7. **Uninstall removes the HKCU `Run` value `ClaudeDashboard`** (the app's Launch at startup), using a deferred `reg.exe delete` custom action. The condition is `REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE`, so an upgrade keeps the user's setting. `Return="ignore"`, because the value may be absent.
8. **GUI tasks** (Task 4) are verified by build + smoke + screenshot + the named pure helpers. **Installer tasks** (Task 5) are verified by a local `cargo wix` build. The controller then runs an install/upgrade/uninstall check on this machine; the user approved this on 2026-10-05.

## Review Focus

1. **An upgrade keeps user data and never downgrades.** `MajorUpgrade` with a downgrade error. `is_newer` returns false for an equal or older version, and a missing component counts as 0. → Task 2 `is_newer_*` tests; Task 5 controller upgrade check (build at the current version, install, rebuild with `--install-version` one patch higher, install over it, confirm `accounts.json` is untouched).
2. **Auto-update never acts when it shouldn't.** That covers: a dev build; `DISABLE_AUTOUPDATER=1`; auto-update off; a draft or prerelease payload; no `ClaudeDashboard-x64.msi` asset; offline or a non-200 response; and a check run inside the 24 h window. → Task 2 `parse_release_*` / `is_due_*`; Task 3 `is_installed_copy_*` / `auto_update_allowed_*`.
3. **The relauncher survives awkward paths,** such as a user profile with a space or an apostrophe (`C:\Users\O'Brien Smith\…`). → Task 3 `relaunch_script_quotes_paths`.
4. **A download that isn't an MSI is never run,** for example a captive-portal HTML page or a truncated file. → Task 3 `msi_magic_*`.
5. **Uninstall leaves no dangling native-messaging keys and no `Run` value, and the release workflow can never replace or create macOS assets or a release.** → Task 5 controller uninstall check; Task 7 `test-release-workflow.sh` covering `release-windows.yml`.

## File structure

```
scripts/
  sync-version.sh          (mod) portable sedi(); + apps/windows Cargo.toml/Cargo.lock/extension manifest
  test-sync-version.sh     (mod) fixtures + assertions for the three Windows files
  release.sh               (mod) git add the three Windows files; sedi for its own sha256 edits
  test-release-workflow.sh (mod) shared guardrail checks over release-linux.yml AND release-windows.yml
.github/workflows/
  ci.yml                   (mod) linux job runs test-sync-version.sh + test-release-workflow.sh
  release-windows.yml      (new) build MSI + extension zip, upload to the existing release
apps/linux/core/src/
  update.rs                (new) is_newer, version_from_tag, UpdateInfo, parse_release, is_due, fetch_latest_release
  settings.rs              (mod) auto_update (default true), last_auto_update_check_unix
  lib.rs                   (mod) pub mod update
apps/windows/
  Cargo.toml               (mod) comment update only (sync now owns the version)
  app/Cargo.toml           (mod) ureq dep; [package.metadata.wix] none needed
  app/wix/main.wxs         (new) per-user MSI definition
  app/wix/com.claude_dashboard.bridge.json (new) shipped host manifest (relative path)
  app/src/updater.rs       (new) install-dir check, auto_update_allowed, msi magic, download, relaunch_script, apply, UpdateState + status text
  app/src/main.rs          (mod) mod updater; background auto-update thread; Updates UI wiring
  app/src/settings_general.rs (mod) auto-update toggle get/set
  app/ui/app.slint         (mod) Updates card: status, Check / Install buttons, auto toggle
  scripts/build-msi.ps1    (new) cargo build --release --locked --workspace + cargo wix → target\wix\ClaudeDashboard-x64.msi
  scripts/pack-extension.ps1 (new) allowlist zip → claude-dashboard-extension.zip
  scripts/test-pack-extension.ps1 (new) asserts zip contents
contract/windows.md        (mod) Registration (installer, relative manifest), Updates, Release
CLAUDE.md                  (mod) Windows build/release commands, Releasing section
```

---

### Task 1: Version sync covers Windows (portable sed) — TDD via the shell test

**Files:**
- Modify: `scripts/sync-version.sh`, `scripts/test-sync-version.sh`, `scripts/release.sh`, `.github/workflows/ci.yml`, `apps/windows/Cargo.toml` (comment only)

**Interfaces:**
- Produces: `sync-version.sh` keeps its CLI (no arguments, reads `/VERSION`) and now also writes three files:
  - `apps/windows/Cargo.toml`: `^version = "X.Y.Z"`, keeping any trailing comment.
  - `apps/windows/Cargo.lock`: the `version` line right after `name = "<member>"`, for members `claude-dashboard`, `claude-dashboard-bridge` and `claude-dashboard-core`.
  - `apps/windows/extension/manifest.json`: `"version": "X.Y.Z"`. Only the top-level `"version"` key; the file has no other key named exactly `version`.

- [ ] **Step 1: Extend the test first.** In `test-sync-version.sh`:
  - `make_fixture`: also `mkdir -p "$tmp/apps/windows/extension"` and copy `apps/windows/Cargo.toml`, `apps/windows/Cargo.lock` and `apps/windows/extension/manifest.json` from `$REPO_ROOT`.
  - Test 1: add these assertions.
    ```bash
    grep -q '^version = "9.9.9"' "$T1/apps/windows/Cargo.toml" && ok "windows Cargo.toml bumped" || ko "windows Cargo.toml bumped"
    grep -q '"version": "9.9.9"' "$T1/apps/windows/extension/manifest.json" && ok "extension manifest bumped" || ko "extension manifest bumped"
    for member in claude-dashboard claude-dashboard-bridge claude-dashboard-core; do
        got="$(awk -v m="$member" '$0 == "name = \"" m "\"" { getline; print; exit }' "$T1/apps/windows/Cargo.lock")"
        [[ "$got" == 'version = "9.9.9"' ]] && ok "windows Cargo.lock bumped ($member)" || ko "windows Cargo.lock bumped ($member)"
    done
    # A third-party crate that happens to share the old version must not move.
    grep -c '^version = "9.9.9"' "$T1/apps/windows/Cargo.lock" | { read n; [[ "$n" -eq 3 ]]; } && ok "only 3 lock entries changed" || ko "only 3 lock entries changed"
    ```
  - Test 2 (idempotency): snapshot and diff the three new files, the same way the others are handled.
  - New Test 6, "no .bak litter": after a run, `find "$T6" -name '*.bak'` must be empty.

  The "only 3 lock entries" assertion assumes no third-party crate in `apps/windows/Cargo.lock` is literally at 9.9.9, which is true. Before writing it, check that the original lock has no non-member at the *current* version. If one exists, assert per member only and drop the count line.
- [ ] **Step 2: Run it and watch it fail.** Run `bash scripts/test-sync-version.sh` in Git Bash. Expected: on this machine it currently errors immediately, because `sed -i ''` is BSD-only. That is part of the RED. Record the output.
- [ ] **Step 3: Implement.** In `sync-version.sh`, add the helper right after `report()`:
  ```bash
  # In-place edit that works on BSD sed (macOS) and GNU sed (Linux, Git Bash):
  # both accept -i with an attached suffix; the backup is removed straight away.
  sedi() {
      local file="${*: -1}"
      sed -i.bak "$@" && rm -f "${file}.bak"
  }
  ```
  Replace every `sed -i ''` with `sedi`; the arguments are otherwise unchanged. Then append sections 7–9:
  ```bash
  # 7. Windows Cargo workspace — [workspace.package] version (trailing comment kept).
  WIN_CARGO_TOML="apps/windows/Cargo.toml"
  sedi "s|^version = \"[^\"]*\"|version = \"${VERSION}\"|" "$WIN_CARGO_TOML"
  report "$WIN_CARGO_TOML" "^version = \"${VERSION}\""

  # 8. Windows lock — the app, the bridge, and the path-dep core all carry the version.
  WIN_CARGO_LOCK="apps/windows/Cargo.lock"
  for member in claude-dashboard claude-dashboard-bridge claude-dashboard-core; do
      sedi "/^name = \"${member}\"$/{n;s|^version = \"[^\"]*\"|version = \"${VERSION}\"|;}" "$WIN_CARGO_LOCK"
  done
  for member in claude-dashboard claude-dashboard-bridge claude-dashboard-core; do
      got="$(awk -v m="$member" '$0 == "name = \"" m "\"" { getline; print; exit }' "$WIN_CARGO_LOCK")"
      if [[ "$got" == "version = \"${VERSION}\"" ]]; then
          echo "  $WIN_CARGO_LOCK ($member) — OK"
      else
          echo "  $WIN_CARGO_LOCK ($member) — FAILED to apply" >&2
          exit 1
      fi
  done

  # 9. Browser extension manifest — Chrome requires 1-4 dot-separated integers.
  EXT_MANIFEST="apps/windows/extension/manifest.json"
  sedi "s|\"version\": \"[^\"]*\"|\"version\": \"${VERSION}\"|" "$EXT_MANIFEST"
  report "$EXT_MANIFEST" "\"version\": \"${VERSION}\""
  ```
  Update the header comment ("Sync the version in VERSION to …") to list the new files.

  On Windows the `sed -i.bak` route can rewrite LF line endings to CRLF. If `git diff` after a sync shows whole-file changes, add a `--binary` flag (GNU sed, Git Bash) inside `sedi` only when `sed --version` reports GNU, and say so in the report. **Do not** reformat the files otherwise.

  In `release.sh`, replace its own four `sed -i ''` uses with an identical local `sedi`, and add `apps/windows/Cargo.toml apps/windows/Cargo.lock apps/windows/extension/manifest.json` to the `git add` in Step 7. Keep the "Every file sync-version.sh writes must be listed here" comment true.

  In `apps/windows/Cargo.toml`, change the comment to `# kept in sync with /VERSION by scripts/sync-version.sh`.

  In `ci.yml`, add a step to the `linux` job after checkout:
  ```yaml
        - name: Release script checks
          run: |
            bash scripts/test-sync-version.sh
            bash scripts/test-release-workflow.sh
  ```
- [ ] **Step 4: Run.** `bash scripts/test-sync-version.sh` (Git Bash) gives all PASS. `bash scripts/test-release-workflow.sh` still gives PASS. `git status` must be clean apart from your edits, because the test works in temp dirs.
- [ ] **Step 5: Commit** with message `build: sync version into the Windows workspace and extension` and the trailer.

---

### Task 2: `core::update` — release decision logic (TDD) + settings fields

**Files:**
- Create: `apps/linux/core/src/update.rs`
- Modify: `apps/linux/core/src/lib.rs` (`pub mod update;`), `apps/linux/core/src/settings.rs`

**Interfaces:**
- Produces (`update.rs`):
  - `pub const REPO_SLUG: &str = "haiz/claude-dashboard";`
  - `pub const RELEASES_URL: &str = "https://api.github.com/repos/haiz/claude-dashboard/releases/latest";`
  - `pub const MSI_ASSET: &str = "ClaudeDashboard-x64.msi";`
  - `pub const CHECK_INTERVAL_S: f64 = 86400.0;`
  - `pub fn is_newer(remote: &str, current: &str) -> bool`
  - `pub fn version_from_tag(tag: &str) -> &str`
  - `#[derive(Debug, Clone, PartialEq)] pub struct UpdateInfo { pub version: String, pub download_url: String, pub body: Option<String> }`
  - `#[derive(Debug, Clone, PartialEq)] pub enum UpdateError { NotARelease, AssetNotFound, Http(String) }` with `Display`: "Unexpected response from GitHub." / "The release has no Windows installer." / the message.
  - `pub fn parse_release(json: &str, current: &str, asset: &str) -> Result<Option<UpdateInfo>, UpdateError>`
  - `pub fn is_due(last_unix: Option<f64>, now_unix: f64, interval_s: f64) -> bool`
  - `pub fn fetch_latest_release(current_version: &str) -> Result<String, UpdateError>` (network; not unit-tested)
- Produces (`settings.rs`):
  - `#[serde(rename = "autoUpdate", default = "default_true")] pub auto_update: bool` (default `true`)
  - `#[serde(rename = "lastAutoUpdateCheck", skip_serializing_if = "Option::is_none", default)] pub last_auto_update_check_unix: Option<f64>`
  - Both are added to `Default` (`auto_update: true`, `last_auto_update_check_unix: None`).

- [ ] **Step 1: Write the failing tests** in `update.rs`:
```rust
//! Release-check decisions (ports UpdateService.swift / apps/linux/lib/update.js).
//! Pure except `fetch_latest_release`; the download and install live in the app.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_compares_numerically() {
        assert!(is_newer("1.19.0", "1.18.1"));
        assert!(is_newer("1.18.10", "1.18.9"), "numeric, not lexical");
        assert!(is_newer("2.0", "1.99.99"));
        assert!(!is_newer("1.18.1", "1.18.1"));
        assert!(!is_newer("1.18.0", "1.18.1"), "never downgrade");
        assert!(!is_newer("1.18", "1.18.0"), "missing component is 0");
        assert!(is_newer("1.18.0.1", "1.18"));
        assert!(!is_newer("", "1.0.0"));
        assert!(!is_newer("garbage", "1.0.0"));
    }

    #[test]
    fn tag_prefix_is_stripped_once() {
        assert_eq!(version_from_tag("v1.2.3"), "1.2.3");
        assert_eq!(version_from_tag("1.2.3"), "1.2.3");
        assert_eq!(version_from_tag("vv1"), "v1");
    }

    fn payload(tag: &str, extra: &str, assets: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","body":"notes","html_url":"https://x"{extra},"assets":[{assets}]}}"#)
    }
    const MSI: &str = r#"{"name":"ClaudeDashboard-x64.msi","browser_download_url":"https://dl/msi"}"#;
    const ZIP: &str = r#"{"name":"ClaudeDashboard.app.zip","browser_download_url":"https://dl/zip"}"#;

    #[test]
    fn parse_release_returns_newer_msi() {
        let got = parse_release(&payload("v1.19.0", "", &format!("{ZIP},{MSI}")), "1.18.1", MSI_ASSET).unwrap();
        assert_eq!(got, Some(UpdateInfo { version: "1.19.0".into(), download_url: "https://dl/msi".into(), body: Some("notes".into()) }));
    }

    #[test]
    fn parse_release_none_when_not_newer() {
        assert_eq!(parse_release(&payload("v1.18.1", "", MSI), "1.18.1", MSI_ASSET).unwrap(), None);
        assert_eq!(parse_release(&payload("v1.0.0", "", MSI), "1.18.1", MSI_ASSET).unwrap(), None);
    }

    #[test]
    fn parse_release_ignores_draft_and_prerelease() {
        assert_eq!(parse_release(&payload("v9.0.0", r#","draft":true"#, MSI), "1.0.0", MSI_ASSET).unwrap(), None);
        assert_eq!(parse_release(&payload("v9.0.0", r#","prerelease":true"#, MSI), "1.0.0", MSI_ASSET).unwrap(), None);
    }

    #[test]
    fn parse_release_newer_without_msi_is_asset_not_found() {
        assert_eq!(parse_release(&payload("v9.0.0", "", ZIP), "1.0.0", MSI_ASSET), Err(UpdateError::AssetNotFound));
    }

    #[test]
    fn parse_release_rejects_non_release() {
        for bad in ["", "{", "[]", r#"{"message":"Not Found"}"#, r#"{"tag_name":5}"#] {
            assert_eq!(parse_release(bad, "1.0.0", MSI_ASSET), Err(UpdateError::NotARelease), "{bad:?}");
        }
    }

    #[test]
    fn is_due_respects_interval() {
        assert!(is_due(None, 1000.0, CHECK_INTERVAL_S));
        assert!(is_due(Some(0.0), 1000.0, CHECK_INTERVAL_S), "0 means never");
        assert!(!is_due(Some(1000.0), 1000.0 + 3600.0, CHECK_INTERVAL_S));
        assert!(is_due(Some(1000.0), 1000.0 + CHECK_INTERVAL_S, CHECK_INTERVAL_S));
        assert!(is_due(Some(5000.0), 1000.0, CHECK_INTERVAL_S), "clock went backwards -> check");
    }
}
```
  Add these to the `settings.rs` tests:
```rust
    #[test]
    fn auto_update_defaults_on_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        fs::write(&p, r#"{"autoRefreshSeconds":60}"#).unwrap();
        let s = load_from(&p);
        assert!(s.auto_update, "missing key -> on (macOS default)");
        assert_eq!(s.last_auto_update_check_unix, None);
        let s2 = Settings { auto_update: false, last_auto_update_check_unix: Some(123.0), ..Settings::default() };
        save_to(&p, &s2).unwrap();
        assert_eq!(load_from(&p), s2);
    }
```
  Also add `auto_update: true, last_auto_update_check_unix: None` to the full `Settings { … }` literal in the existing `roundtrip` test.
- [ ] **Step 2: Run it and watch it fail.** Run `cd apps/linux && cargo test -p claude-dashboard-core update settings`. Expected: compile errors.
- [ ] **Step 3: Implement:**
```rust
use serde_json::Value;

pub const REPO_SLUG: &str = "haiz/claude-dashboard";
pub const RELEASES_URL: &str = "https://api.github.com/repos/haiz/claude-dashboard/releases/latest";
pub const MSI_ASSET: &str = "ClaudeDashboard-x64.msi";
pub const CHECK_INTERVAL_S: f64 = 86400.0;

fn parts(v: &str) -> Vec<u64> {
    v.split('.').filter_map(|p| p.parse::<u64>().ok()).collect()
}

/// Component-wise numeric compare; a missing component reads as 0 and a
/// non-numeric one is dropped (UpdateService.isNewer's compactMap).
pub fn is_newer(remote: &str, current: &str) -> bool {
    let (r, c) = (parts(remote), parts(current));
    for i in 0..r.len().max(c.len()) {
        let (rv, cv) = (r.get(i).copied().unwrap_or(0), c.get(i).copied().unwrap_or(0));
        if rv != cv {
            return rv > cv;
        }
    }
    false
}

pub fn version_from_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

#[derive(Debug, Clone, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub download_url: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateError {
    NotARelease,
    AssetNotFound,
    Http(String),
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::NotARelease => write!(f, "Unexpected response from GitHub."),
            UpdateError::AssetNotFound => write!(f, "The release has no Windows installer."),
            UpdateError::Http(m) => write!(f, "{m}"),
        }
    }
}

/// `Ok(None)` = up to date (or a draft/prerelease, which `releases/latest`
/// should never return but is ignored defensively).
pub fn parse_release(json: &str, current: &str, asset: &str) -> Result<Option<UpdateInfo>, UpdateError> {
    let v: Value = serde_json::from_str(json).map_err(|_| UpdateError::NotARelease)?;
    let tag = v.get("tag_name").and_then(Value::as_str).ok_or(UpdateError::NotARelease)?;
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    if flag("draft") || flag("prerelease") {
        return Ok(None);
    }
    let version = version_from_tag(tag);
    if !is_newer(version, current) {
        return Ok(None);
    }
    let url = v
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|x| x.get("name").and_then(Value::as_str) == Some(asset)))
        .and_then(|x| x.get("browser_download_url").and_then(Value::as_str))
        .ok_or(UpdateError::AssetNotFound)?;
    Ok(Some(UpdateInfo {
        version: version.to_string(),
        download_url: url.to_string(),
        body: v.get("body").and_then(Value::as_str).map(str::to_string),
    }))
}

/// Never checked, or `interval_s` has passed, or the clock moved backwards.
pub fn is_due(last_unix: Option<f64>, now_unix: f64, interval_s: f64) -> bool {
    match last_unix {
        None => true,
        Some(l) if l <= 0.0 => true,
        Some(l) => now_unix < l || now_unix - l >= interval_s,
    }
}

/// GET `releases/latest`. Non-200 and transport failures become `Http`.
pub fn fetch_latest_release(current_version: &str) -> Result<String, UpdateError> {
    let resp = ureq::get(RELEASES_URL)
        .timeout(std::time::Duration::from_secs(20))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .set("User-Agent", &format!("claude-dashboard/{current_version}"))
        .call()
        .map_err(|e| UpdateError::Http(format!("Update check failed: {e}")))?;
    if resp.status() != 200 {
        return Err(UpdateError::Http(format!("Update check failed: HTTP {}", resp.status())));
    }
    resp.into_string().map_err(|e| UpdateError::Http(format!("Update check failed: {e}")))
}
```
  In `settings.rs`, add `fn default_true() -> bool { true }`, the two fields with the serde attributes above, and the `Default` values.
- [ ] **Step 4: Run.** Run `cd apps/linux && cargo test -p claude-dashboard-core` + clippy; then `cd apps/windows && cargo test --workspace` + clippy. If a full `Settings { … }` literal elsewhere breaks, add the two fields there. Expected: green.
- [ ] **Step 5: Commit** with message `feat(core): release update check logic and auto-update settings` and the trailer.

---

### Task 3: app `updater.rs` — guards, download, relaunch (TDD)

**Files:**
- Create: `apps/windows/app/src/updater.rs`
- Modify: `apps/windows/app/src/main.rs` (`mod updater;`), `apps/windows/app/Cargo.toml` (add `ureq = "2"` under `[dependencies]`)

**Interfaces:**
- Consumes: `claude_dashboard_core::update::{fetch_latest_release, parse_release, UpdateInfo, UpdateError, MSI_ASSET}`.
- Produces:
  - `pub fn install_dir(local_appdata: &Path) -> PathBuf` → `<local_appdata>\Programs\ClaudeDashboard`
  - `pub fn is_installed_copy(exe: &Path, local_appdata: &Path) -> bool` — exe == `install_dir\claude-dashboard.exe`, compared case-insensitively on the full path string, with `/` normalised to `\`.
  - `pub fn auto_update_allowed(enabled: bool, installed: bool, disable_env: Option<&str>) -> bool` — `enabled && installed && disable_env != Some("1")`
  - `pub const MSI_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]; pub const MAX_MSI_BYTES: u64 = 200 * 1024 * 1024;`
  - `pub fn looks_like_msi(head: &[u8]) -> bool`
  - `pub fn ps_quote(s: &str) -> String` → `'…'` with `'` doubled
  - `pub fn relaunch_script(pid: u32, msi: &Path, exe: &Path) -> String`
  - `pub fn download(info: &UpdateInfo, current_version: &str) -> Result<PathBuf, String>` → `%TEMP%\ClaudeDashboard-update-<version>-<uuid>.msi`. It streams with the size cap, checks the magic, and deletes the file on any failure.
  - `pub fn apply(msi: &Path, exe: &Path) -> Result<(), String>` — spawns the relauncher detached and hidden; the caller then quits.
  - `pub fn check(current_version: &str) -> Result<Option<UpdateInfo>, String>` — `fetch_latest_release` + `parse_release(.., MSI_ASSET)`, errors mapped through `to_string()`.
  - `#[derive(Debug, Clone, PartialEq)] pub enum UpdateState { Idle, Checking, UpToDate, Available(UpdateInfo), Downloading, Installing, DevBuild(Option<UpdateInfo>), Failed(String) }`
  - `pub fn status_text(state: &UpdateState, current: &str) -> String`

`status_text` strings (exact):

| State | Text |
|---|---|
| Idle | `Version {current}` |
| Checking | `Checking for updates…` |
| UpToDate | `Up to date (version {current})` |
| Available(i) | `Version {i.version} is available` |
| Downloading | `Downloading update…` |
| Installing | `Installing update…` |
| DevBuild(None) | `Development build — install the MSI to update` |
| DevBuild(Some(i)) | `Version {i.version} is available — install the MSI to update` |
| Failed(m) | `{m}` |

`relaunch_script` (single line joined with `; `; `{P}`, `{MSI}` and `{EXE}` are the `ps_quote`d values):
```
$ErrorActionPreference = 'SilentlyContinue'; Wait-Process -Id {pid} -Timeout 30; Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', {MSI}, '/passive', '/norestart') -Wait; Start-Process -FilePath {EXE}
```
`apply` runs `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -Command <script>` with `creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)` (`0x0800_0000 | 0x0000_0008`), stdin, stdout and stderr all null, and no Job Object. `powershell.exe` comes from `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`.

Note on `msiexec`: with `-ArgumentList` given as an array, PowerShell 5.1 joins the elements with spaces and does **not** quote them. So the MSI path element must carry its own double quotes. Build it as `ps_quote(&format!("\"{}\"", msi.display()))`. The test asserts that form.

- [ ] **Step 1: Write the failing tests:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::update::UpdateInfo;

    const LAD: &str = r"C:\Users\u\AppData\Local";

    #[test]
    fn installed_copy_detection_is_case_insensitive() {
        let lad = Path::new(LAD);
        assert!(is_installed_copy(Path::new(r"C:\Users\u\AppData\Local\Programs\ClaudeDashboard\claude-dashboard.exe"), lad));
        assert!(is_installed_copy(Path::new(r"c:\users\U\appdata\local\programs\claudedashboard\CLAUDE-DASHBOARD.EXE"), lad));
        assert!(!is_installed_copy(Path::new(r"G:\code\apps\windows\target\debug\claude-dashboard.exe"), lad));
        assert!(!is_installed_copy(Path::new(r"C:\Users\u\AppData\Local\Programs\ClaudeDashboard\other.exe"), lad));
        assert_eq!(install_dir(lad), PathBuf::from(r"C:\Users\u\AppData\Local\Programs\ClaudeDashboard"));
    }

    #[test]
    fn auto_update_allowed_needs_all_three() {
        assert!(auto_update_allowed(true, true, None));
        assert!(auto_update_allowed(true, true, Some("0")));
        assert!(!auto_update_allowed(false, true, None));
        assert!(!auto_update_allowed(true, false, None), "dev build never auto-installs");
        assert!(!auto_update_allowed(true, true, Some("1")));
    }

    #[test]
    fn msi_magic_accepts_ole_and_rejects_html() {
        let mut ok = MSI_MAGIC.to_vec();
        ok.extend_from_slice(b"rest");
        assert!(looks_like_msi(&ok));
        assert!(!looks_like_msi(b"<!DOCTYPE html><html>"));
        assert!(!looks_like_msi(&MSI_MAGIC[..4]), "truncated");
        assert!(!looks_like_msi(b""));
    }

    #[test]
    fn ps_quote_doubles_single_quotes() {
        assert_eq!(ps_quote("plain"), "'plain'");
        assert_eq!(ps_quote("O'Brien"), "'O''Brien'");
        assert_eq!(ps_quote(""), "''");
    }

    #[test]
    fn relaunch_script_quotes_paths() {
        let s = relaunch_script(
            4242,
            Path::new(r"C:\Users\O'Brien Smith\AppData\Local\Temp\ClaudeDashboard-update-1.19.0-x.msi"),
            Path::new(r"C:\Users\O'Brien Smith\AppData\Local\Programs\ClaudeDashboard\claude-dashboard.exe"),
        );
        assert!(s.contains("Wait-Process -Id 4242 -Timeout 30"));
        assert!(s.contains(r#"'"C:\Users\O''Brien Smith\AppData\Local\Temp\ClaudeDashboard-update-1.19.0-x.msi"'"#), "{s}");
        assert!(s.contains(r"-FilePath 'C:\Users\O''Brien Smith\AppData\Local\Programs\ClaudeDashboard\claude-dashboard.exe'"), "{s}");
        assert!(s.contains("'/passive', '/norestart') -Wait"));
        // msiexec runs before the relaunch.
        assert!(s.find("msiexec").unwrap() < s.rfind("Start-Process").unwrap());
    }

    fn info() -> UpdateInfo {
        UpdateInfo { version: "1.19.0".into(), download_url: "u".into(), body: None }
    }

    #[test]
    fn status_texts() {
        assert_eq!(status_text(&UpdateState::Idle, "1.18.1"), "Version 1.18.1");
        assert_eq!(status_text(&UpdateState::UpToDate, "1.18.1"), "Up to date (version 1.18.1)");
        assert_eq!(status_text(&UpdateState::Available(info()), "1.18.1"), "Version 1.19.0 is available");
        assert_eq!(status_text(&UpdateState::DevBuild(None), "1.18.1"), "Development build — install the MSI to update");
        assert_eq!(status_text(&UpdateState::DevBuild(Some(info())), "1.18.1"), "Version 1.19.0 is available — install the MSI to update");
        assert_eq!(status_text(&UpdateState::Failed("boom".into()), "1.18.1"), "boom");
        assert_eq!(status_text(&UpdateState::Checking, "x"), "Checking for updates…");
    }
}
```
- [ ] **Step 2: Run it and watch it fail.** Run `cargo test -p claude-dashboard updater`. Expected: compile errors.
- [ ] **Step 3: Implement** per the interfaces. In `download`:
  - Use `ureq::get(&info.download_url).set("User-Agent", …).timeout(300 s).call()`, and read via `resp.into_reader().take(MAX_MSI_BYTES + 1)` into a `File`.
  - If more than `MAX_MSI_BYTES` arrive, delete the file and return `Err("The update is too large.")`.
  - Re-open the file, read 8 bytes, and if `!looks_like_msi`, delete it and return `Err("The downloaded update is not a Windows installer.")`.
  - The temp dir is `std::env::temp_dir()`; the uuid comes from the `uuid` crate (already a dep).

  Mark items not yet used by `main.rs` with `#[allow(dead_code)] // used by main.rs (Task 4)`; Task 4 removes those allows.
- [ ] **Step 4: Run.** Run `cargo test --workspace` + clippy. Expected: green.
- [ ] **Step 5: Commit** with message `feat(windows): update download, guard and relaunch helpers` and the trailer.

---

### Task 4: Settings › General › Updates UI + background auto-update (GUI)

**Files:**
- Modify: `apps/windows/app/ui/app.slint` (the Updates `SettingsCard` in `GeneralSettingsPane`, ~lines 264–273; the AppWindow props/callbacks), `apps/windows/app/src/main.rs`, `apps/windows/app/src/settings_general.rs`, `apps/windows/app/src/updater.rs` (remove the Task 3 allows)

**Interfaces:**
- Consumes: Task 2 `settings::{load, update}` + `Settings.{auto_update, last_auto_update_check_unix}`, `update::{is_due, CHECK_INTERVAL_S}`; Task 3 `updater::*`.
- Produces:
  - AppWindow: `in property <string> update-status; in property <bool> update-can-install; in property <bool> update-busy; in-out property <bool> auto-update: true; callback check-updates(); callback install-update(); callback set-auto-update(bool);`
  - GeneralSettingsPane: the matching `in` props and callbacks.
  - `settings_general::{auto_update_enabled() -> bool, set_auto_update(on: bool) -> Result<(), String>}`, both via `settings::load` / `settings::update`.

Behaviour (in `main.rs`, in a new `install_updates(app: &AppWindow)` function next to the other installs):
- **State.** A `thread_local!` `RefCell<UpdateState>` plus the last `UpdateInfo`. A UI-thread function `fn show(app, state)` sets `update-status = status_text(state, APP_VERSION)`, `update-can-install = matches!(state, Available(_))` and `update-busy = matches!(state, Checking | Downloading | Installing)`.
- **Installed-copy check.** `installed = updater::is_installed_copy(&current_exe, &local_appdata)`, computed once at install time. `LOCALAPPDATA` comes from the env.
- **`check-updates`.** Show `Checking`, then on a worker call `updater::check(APP_VERSION)`. The result maps to: `Ok(Some(i))` → `Available(i)` if installed, else `DevBuild(Some(i))`; `Ok(None)` → `UpToDate` if installed, else `DevBuild(None)`; `Err(m)` → `Failed(m)`.
- **`install-update`.** Only when the state is `Available(i)` and installed. Show `Downloading`. On a worker: `download(&i, APP_VERSION)` → on Ok show `Installing` and call `apply(&msi, &current_exe)` → on Ok call `slint::quit_event_loop()` via `invoke_from_event_loop`. Any Err → `Failed(m)`.
- **`set-auto-update(on)`.** Set the prop, then call `settings_general::set_auto_update(on)` on a worker.
- **Background thread,** started once from `main()` and skipped under `CLAUDE_DASHBOARD_SMOKE` and `CLAUDE_DASHBOARD_FAKE_ROWS`. It loops: `if auto_update_allowed(settings.auto_update, installed, env DISABLE_AUTOUPDATER) && is_due(settings.last_auto_update_check_unix, now, CHECK_INTERVAL_S)`, then:
  1. Stamp `last_auto_update_check_unix = now` via `settings::update`.
  2. Check. On `Ok(Some(i))`: show `Downloading`, `download(&i, APP_VERSION)`, show `Installing`, `apply`, quit (Ruling 4). On `Ok(None)`: show `UpToDate`. On error: `eprintln!` and leave the state alone.

  Then it sleeps 3600 s using `std::sync::mpsc::Receiver::recv_timeout` on a channel whose sender is dropped at shutdown, so the thread never keeps the process alive. A detached thread is fine too, since the event-loop exit ends the process.
- **Startup.** Read `auto_update_enabled()` on a worker into `auto-update`, and show `Idle`, or `DevBuild(None)` when not installed.

UI (the Updates card, replacing "Check for updates / Coming soon"):
```slint
        GroupHeader { text: "Updates"; }
        SettingsCard {
            height: upd.preferred-height + 24px;
            upd := VerticalLayout {
                padding: 12px;
                spacing: 10px;
                HorizontalLayout {
                    spacing: 10px;
                    Text { text: root.update-status; color: Theme.text-primary; vertical-alignment: center; horizontal-stretch: 1; wrap: word-wrap; }
                    if root.update-can-install: IntervalChip { label: "Install"; active: true; clicked => { root.install-update(); } }
                    IntervalChip { label: root.update-busy ? "Working…" : "Check for Updates"; active: false;
                                   clicked => { if !root.update-busy { root.check-updates(); } } }
                }
                HorizontalLayout {
                    Text { text: "Install updates automatically"; color: Theme.text-primary; vertical-alignment: center; horizontal-stretch: 1; }
                    Rectangle {
                        width: 40px; height: 22px;
                        border-radius: 11px;
                        background: root.auto-update ? Theme.accent : Theme.card-border;
                        Rectangle { width: 16px; height: 16px; border-radius: 8px; background: #ffffff; y: 3px;
                                    x: root.auto-update ? parent.width - self.width - 3px : 3px; }
                        TouchArea { clicked => { root.set-auto-update(!root.auto-update); } }
                    }
                }
                Text { text: "Checks GitHub once a day and installs new releases."; color: Theme.text-secondary; font-size: 11px; }
            }
        }
```
This mirrors the Launch-at-startup toggle already in the pane; reuse its exact structure if it differs.

- [ ] **Step 1: Write the failing test** for the settings glue in `settings_general.rs` tests (testenv lock + temp APPDATA, the same pattern as the existing tests):
```rust
    #[test]
    fn auto_update_toggle_persists() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        assert!(auto_update_enabled(), "default on");
        set_auto_update(false).unwrap();
        assert!(!auto_update_enabled());
        assert!(!settings::load().auto_update);
    }
```
  Run it and watch it fail.
- [ ] **Step 2: Implement** the glue, the UI and `install_updates`. Use the Edit tool for `.slint`. **Report requirement:** paste the new Updates card and the AppWindow ↔ GeneralSettingsPane wiring lines verbatim.
- [ ] **Step 3: Build and test.** Run `cargo test --workspace` + clippy (no `dead_code` allowances left in `updater.rs`) + `cargo build -p claude-dashboard`.
- [ ] **Step 4: Smoke run** with `CLAUDE_DASHBOARD_SMOKE=1 CLAUDE_DASHBOARD_FAKE_ROWS=1`: self-exits, no panic, and no network access. The background updater is skipped under these flags.
- [ ] **Step 5: Commit** with message `feat(windows): Updates pane and daily auto-update` and the trailer.

---

### Task 5: Per-user MSI (`cargo-wix`) + build script

**Files:**
- Create: `apps/windows/app/wix/main.wxs`, `apps/windows/app/wix/com.claude_dashboard.bridge.json`, `apps/windows/scripts/build-msi.ps1`
- Modify: `apps/windows/.gitignore` (only if `target/wix` isn't already covered by `target/`)

**Interfaces:**
- Produces: `apps/windows/scripts/build-msi.ps1 [-WixDir <path>] [-InstallVersion <x.y.z>]` → `apps/windows/target/wix/ClaudeDashboard-x64.msi`. Exit code ≠ 0 on any failure. Task 7's workflow and the controller check consume it.

- [ ] **Step 1: Write the manifest** `apps/windows/app/wix/com.claude_dashboard.bridge.json` (UTF-8, no BOM):
```json
{
  "name": "com.claude_dashboard.bridge",
  "description": "Claude Dashboard native messaging host",
  "path": "claude-dashboard-bridge.exe",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://cadpjcfajhlgdaipepkdojcehfmkkedh/"]
}
```
- [ ] **Step 2: Write `main.wxs`** (WiX v3 schema):
```xml
<?xml version="1.0" encoding="utf-8"?>
<!-- Per-user MSI for Claude Dashboard (contract/windows.md "Installer").
     Built by apps/windows/scripts/build-msi.ps1 via cargo-wix, which defines
     $(var.Version) and $(var.CargoTargetBinDir). No admin: everything lives in
     the user's profile and HKCU. User data (%APPDATA% / %LOCALAPPDATA%
     \claude-dashboard) is never referenced here, so upgrade and uninstall keep it. -->
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Product Id="*" Name="Claude Dashboard" Language="1033" Version="$(var.Version)"
           Manufacturer="Claude Dashboard" UpgradeCode="173B2BBC-0CBB-472C-B3BF-E88EE21C4D58">
    <Package InstallerVersion="500" Compressed="yes" InstallScope="perUser"
             InstallPrivileges="limited" Platform="x64" Description="Claude Dashboard" />
    <MajorUpgrade Schedule="afterInstallInitialize"
                  DowngradeErrorMessage="A newer version of Claude Dashboard is already installed." />
    <MediaTemplate EmbedCab="yes" />

    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="LocalAppDataFolder">
        <Directory Id="ProgramsDir" Name="Programs">
          <Directory Id="INSTALLDIR" Name="ClaudeDashboard" />
        </Directory>
      </Directory>
      <Directory Id="ProgramMenuFolder" />
    </Directory>

    <DirectoryRef Id="INSTALLDIR">
      <Component Id="AppFiles" Guid="*" Win64="yes">
        <File Id="AppExe" Source="$(var.CargoTargetBinDir)\claude-dashboard.exe" />
        <File Id="BridgeExe" Source="$(var.CargoTargetBinDir)\claude-dashboard-bridge.exe" />
        <File Id="HostManifest" Name="com.claude_dashboard.bridge.json"
              Source="$(sys.SOURCEFILEDIR)com.claude_dashboard.bridge.json" />
        <RegistryValue Root="HKCU" Key="Software\ClaudeDashboard" Name="Installed"
                       Type="integer" Value="1" KeyPath="yes" />
        <RemoveFolder Id="RemoveInstallDir" Directory="INSTALLDIR" On="uninstall" />
        <RemoveFolder Id="RemoveProgramsDir" Directory="ProgramsDir" On="uninstall" />
      </Component>
      <Component Id="NativeMessagingHost" Guid="*" Win64="yes">
        <RegistryValue Root="HKCU" Key="Software\Google\Chrome\NativeMessagingHosts\com.claude_dashboard.bridge"
                       Type="string" Value="[INSTALLDIR]com.claude_dashboard.bridge.json" KeyPath="yes" />
        <RegistryValue Root="HKCU" Key="Software\Microsoft\Edge\NativeMessagingHosts\com.claude_dashboard.bridge"
                       Type="string" Value="[INSTALLDIR]com.claude_dashboard.bridge.json" />
      </Component>
    </DirectoryRef>

    <DirectoryRef Id="ProgramMenuFolder">
      <Component Id="StartMenuShortcut" Guid="*">
        <Shortcut Id="AppShortcut" Name="Claude Dashboard" Target="[INSTALLDIR]claude-dashboard.exe"
                  WorkingDirectory="INSTALLDIR" />
        <RegistryValue Root="HKCU" Key="Software\ClaudeDashboard" Name="StartMenuShortcut"
                       Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>

    <!-- Ruling 7: drop the app's Launch-at-startup value on a real uninstall
         (not on the uninstall half of an upgrade). The value may be absent. -->
    <CustomAction Id="RemoveRunValue" Directory="INSTALLDIR" Execute="deferred" Impersonate="yes" Return="ignore"
                  ExeCommand="&quot;[System64Folder]reg.exe&quot; delete &quot;HKCU\Software\Microsoft\Windows\CurrentVersion\Run&quot; /v ClaudeDashboard /f" />
    <InstallExecuteSequence>
      <Custom Action="RemoveRunValue" Before="RemoveFiles">REMOVE="ALL" AND NOT UPGRADINGPRODUCTCODE</Custom>
    </InstallExecuteSequence>

    <Feature Id="Main" Title="Claude Dashboard" Level="1">
      <ComponentRef Id="AppFiles" />
      <ComponentRef Id="NativeMessagingHost" />
      <ComponentRef Id="StartMenuShortcut" />
    </Feature>
  </Product>
</Wix>
```
  If `light` reports ICE errors (warnings like ICE91 are expected and fine), fix the `.wxs` and record each change in the report. Don't suppress an ICE error with `-sice` unless it is a documented false positive for per-user installs; name it in the report.
- [ ] **Step 3: Write `build-msi.ps1`:**
```powershell
<#
.SYNOPSIS
  Builds apps/windows/target/wix/ClaudeDashboard-x64.msi (release binaries + cargo-wix).
.PARAMETER WixDir
  WiX 3.14 root (contains bin\candle.exe). Defaults to $env:WIX, then
  $env:LOCALAPPDATA\Programs\wix314.
.PARAMETER InstallVersion
  Override the MSI version (upgrade testing only); defaults to the Cargo version.
#>
param(
    [string] $WixDir = $(if ($env:WIX) { $env:WIX } else { Join-Path $env:LOCALAPPDATA 'Programs\wix314' }),
    [string] $InstallVersion = ''
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..')
if (-not (Test-Path (Join-Path $WixDir 'bin\candle.exe'))) { throw "WiX not found: $WixDir\bin\candle.exe" }
$env:WIX = (Resolve-Path $WixDir).Path
Push-Location $root
try {
    cargo build --release --locked --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $out = Join-Path $root 'target\wix\ClaudeDashboard-x64.msi'
    New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
    $wixArgs = @('wix', '--no-build', '--nocapture', '--package', 'claude-dashboard', '--output', $out)
    if ($InstallVersion) { $wixArgs += @('--install-version', $InstallVersion) }
    cargo @wixArgs
    if ($LASTEXITCODE -ne 0) { throw "cargo wix failed" }
    Write-Host "Built $out"
} finally {
    Pop-Location
}
```
  `cargo wix` looks for `wix\main.wxs` in the package directory (`apps/windows/app`), which is where Step 2 put it. If cargo-wix 0.3.9 needs a different flag name on this machine (check `cargo wix --help`), adapt and record it.
- [ ] **Step 4: Build.** Run `powershell -NoProfile -ExecutionPolicy Bypass -File apps\windows\scripts\build-msi.ps1`. Expected: `target\wix\ClaudeDashboard-x64.msi` exists. Inspect it without installing: `msiexec /a` is not needed. Instead run `Get-Item` for its size and `& "$env:WIX\bin\dark.exe" <msi> -o $env:TEMP\cd.wxs` (WiX decompiler), then confirm the decompiled output contains the three files, both HKCU NativeMessagingHosts keys, `InstallScope`/`ALLUSERS` per-user, and the shortcut. Paste the relevant decompiled lines into the report. **Do not run `msiexec /i`**: installing is the controller's job.
- [ ] **Step 5: Run the workspace tests + clippy** (unchanged code, sanity check).
- [ ] **Step 6: Commit** with message `build(windows): per-user MSI via cargo-wix` and the trailer.

**Controller install check (after the task review, before Task 6).** Run this on this machine; the user approved it:
1. Run `msiexec /i <msi> /passive` and verify:
   - the files are in `%LOCALAPPDATA%\Programs\ClaudeDashboard`;
   - both HKCU NativeMessagingHosts default values equal `<installdir>\com.claude_dashboard.bridge.json`;
   - the Start Menu shortcut exists;
   - the app launches from the shortcut.
2. Note the hash of `%APPDATA%\claude-dashboard\accounts.json`.
3. Run `build-msi.ps1 -InstallVersion <next patch>` and install it with `/passive`. Verify one entry in Apps & features, the version updated, and the `accounts.json` hash unchanged.
4. Run `msiexec /x <msi> /passive`. Verify the install dir is gone, both NM keys are gone, the `Run` value `ClaudeDashboard` is gone, and `%APPDATA%\claude-dashboard` is still there.
5. Ledger the results. If the user had the dev host registered via `register-dev-host.ps1`, the uninstall removes those same keys. Tell the user to re-run that script for development.

---

### Task 6: Extension zip

**Files:**
- Create: `apps/windows/scripts/pack-extension.ps1`, `apps/windows/scripts/test-pack-extension.ps1`

**Interfaces:**
- Produces: `pack-extension.ps1 -Out <path>`, which writes a zip whose root holds exactly the allowlist: `manifest.json`, `background.js`, `popup.html`, `popup.js`, `lib/extension-id.js`, `lib/status.js`, `lib/sync.js`. It writes nothing else: no `tests/`, `scripts/`, `package.json` or `*.pem`. Task 7 consumes it.

- [ ] **Step 1: Write the test** `test-pack-extension.ps1`. It packs to a temp path, opens the zip with `System.IO.Compression.ZipFile`, and asserts that the entry names (with `\` normalised to `/`) sorted equal the allowlist sorted. It also asserts that `manifest.json` inside parses as JSON and its `version` equals the repo's `manifest.json` version. It exits 1 on any mismatch and prints `PASS` on success:
```powershell
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$expected = @('background.js','lib/extension-id.js','lib/status.js','lib/sync.js','manifest.json','popup.html','popup.js') | Sort-Object
$out = Join-Path $env:TEMP ("cd-ext-test-" + [guid]::NewGuid() + ".zip")
& (Join-Path $PSScriptRoot 'pack-extension.ps1') -Out $out
$zip = [System.IO.Compression.ZipFile]::OpenRead($out)
try {
    $names = $zip.Entries | ForEach-Object { $_.FullName -replace '\\','/' } | Where-Object { -not $_.EndsWith('/') } | Sort-Object
    if (($names -join ',') -ne ($expected -join ',')) { Write-Error "zip entries: $($names -join ', ')"; exit 1 }
    $reader = New-Object System.IO.StreamReader(($zip.GetEntry('manifest.json')).Open())
    $inner = $reader.ReadToEnd() | ConvertFrom-Json; $reader.Close()
    $repo = Get-Content (Join-Path $PSScriptRoot '..\extension\manifest.json') -Raw | ConvertFrom-Json
    if ($inner.version -ne $repo.version) { Write-Error "version $($inner.version) != $($repo.version)"; exit 1 }
} finally { $zip.Dispose(); Remove-Item $out -ErrorAction SilentlyContinue }
Write-Host 'PASS'
```
  Run it and watch it fail (the pack script is missing).
- [ ] **Step 2: Implement `pack-extension.ps1`.** Use `param([Parameter(Mandatory)] [string] $Out)` and `$ErrorActionPreference='Stop'`.
  - Stage the allowlist files into a temp dir, keeping `lib\`.
  - Fail if any allowlist file is missing.
  - Run `Compress-Archive -Path "$stage\*" -DestinationPath $Out -Force`, then remove the staging dir.
  - Use the allowlist only; never glob the extension directory, so a local `extension-private.pem` can never be shipped.
- [ ] **Step 3: Run.** Run `powershell -NoProfile -ExecutionPolicy Bypass -File apps\windows\scripts\test-pack-extension.ps1`. Expected: PASS. Also run `cd apps/windows/extension && node --test`, which should still pass.
- [ ] **Step 4: Commit** with message `build(windows): package the browser extension zip` and the trailer.

---

### Task 7: `release-windows.yml` + guardrail test over both workflows

**Files:**
- Create: `.github/workflows/release-windows.yml`
- Modify: `scripts/test-release-workflow.sh`

**Interfaces:**
- Consumes: `apps/windows/scripts/build-msi.ps1` (Task 5) and `apps/windows/scripts/pack-extension.ps1` (Task 6).

- [ ] **Step 1: Generalise the test first.** Restructure `test-release-workflow.sh` as follows:
  - Wrap every guardrail check (YAML parse, STRIPPED copy, guardrails 1–5, the `ref:` pin, `--locked` count ≥ 2) in a function `check_common <workflow-file>`. Keep every existing assertion and comment.
  - The guardrail-3 "names the macOS artifacts" checks stay common.
  - Linux-only extras move to `check_linux`: the per-arch asset path, `ubuntu-24.04-arm` and `fail-fast: false`.
  - Add `check_windows`:
    ```bash
    check_windows() {
        local wf=.github/workflows/release-windows.yml
        grep -q 'runs-on: windows-latest' "$STRIPPED" || fail "windows: not on windows-latest"
        grep -qF 'apps/windows/target/wix/ClaudeDashboard-x64.msi' "$STRIPPED" || fail "windows: upload does not name the exact MSI path"
        grep -qF '.build/claude-dashboard-extension.zip' "$STRIPPED" || fail "windows: upload does not name the exact extension zip path"
        grep -q '6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31' "$STRIPPED" || fail "windows: WiX download is not pinned by SHA-256"
        grep -q 'cargo install cargo-wix --locked --version 0.3.9' "$STRIPPED" || fail "windows: cargo-wix not pinned"
    }
    ```
  - Run `check_common` + `check_linux` on `release-linux.yml`, then `check_common` + `check_windows` on `release-windows.yml`, and print one PASS line per workflow.
  - `STRIPPED` is recomputed per workflow inside `check_common`, and the trap cleans every temp file.

  Run it: the Linux checks pass and the Windows ones FAIL ("does not exist"). That is the RED.
- [ ] **Step 2: Write the workflow:**
```yaml
name: release-windows

# Same five guardrails as release-linux.yml (scripts/test-release-workflow.sh
# enforces them on both): published-release trigger only; upload only — never
# create or replace a release; never touch the macOS artifacts; never write
# notes; never commit. --clobber is safe while no Windows asset checksum is
# pinned in any manifest (same caveat as release-linux.yml).
on:
  release:
    types: [published]
  workflow_dispatch:
    inputs:
      tag:
        description: "Existing release tag to build from and upload to (e.g. v1.19.0). Retry path only."
        required: true

permissions:
  contents: write

jobs:
  build:
    concurrency:
      group: release-windows-${{ github.event.release.tag_name || inputs.tag }}
      cancel-in-progress: false
    runs-on: windows-latest
    defaults:
      run:
        shell: pwsh
    steps:
      - name: Resolve tag
        id: resolve
        env:
          EVENT_TAG: ${{ github.event.release.tag_name }}
          DISPATCH_TAG: ${{ inputs.tag }}
        run: |
          $tag = if ($env:GITHUB_EVENT_NAME -eq 'release') { $env:EVENT_TAG } else { $env:DISPATCH_TAG }
          "tag=$tag" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8

      - uses: actions/checkout@v4
        with:
          ref: ${{ steps.resolve.outputs.tag }}
          persist-credentials: false

      - name: Install pinned toolchain
        run: cd apps/windows; rustup toolchain install --no-self-update

      - name: Clippy
        run: cd apps/windows; cargo clippy --workspace --all-targets --locked -- -D warnings

      - name: Test
        run: cd apps/windows; cargo test --workspace --locked

      - name: Install WiX 3.14 (pinned)
        run: |
          $zip = "$env:RUNNER_TEMP\wix314-binaries.zip"
          Invoke-WebRequest -Uri 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip' -OutFile $zip
          $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
          if ($hash -ne '6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31') { throw "WiX zip hash mismatch: $hash" }
          $wix = "$env:RUNNER_TEMP\wix314"
          Expand-Archive -Path $zip -DestinationPath "$wix\bin" -Force
          "WIX=$wix\" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8

      - name: Install cargo-wix
        run: cd apps/windows; cargo install cargo-wix --locked --version 0.3.9

      - name: Build MSI
        run: ./apps/windows/scripts/build-msi.ps1

      - name: Pack extension
        run: |
          New-Item -ItemType Directory -Force .build | Out-Null
          ./apps/windows/scripts/pack-extension.ps1 -Out .build/claude-dashboard-extension.zip

      - name: Upload to release
        env:
          GH_TOKEN: ${{ github.token }}
          TAG: ${{ steps.resolve.outputs.tag }}
        run: gh release upload "$env:TAG" apps/windows/target/wix/ClaudeDashboard-x64.msi .build/claude-dashboard-extension.zip --clobber
```
  `build-msi.ps1` already passes `--locked` to `cargo build`. The `--locked` count in the workflow comes from the clippy, test and cargo-install lines.
- [ ] **Step 3: Run** `bash scripts/test-release-workflow.sh`. Expected: two PASS lines. If YAML parsing is skipped (no PyYAML), that's fine. On this machine `python3` is a Store stub, so the skip branch must still print "skip" and not fail. Verify that, and fix the probe if the stub makes `python3 -c "import yaml"` behave oddly, e.g. by checking `python3 -c "import sys"` first.
- [ ] **Step 4: Commit** with message `ci: release-windows workflow uploads the MSI and extension zip` and the trailer.

---

### Task 8: Contract, CLAUDE.md, final verification

**Files:**
- Modify: `contract/windows.md`, `CLAUDE.md`

- [ ] **Step 1: `contract/windows.md`.**
  - Extend **Registration**: the MSI ships a static manifest with a relative `path` (Ruling 2) and registers the same two HKCU keys; `register-dev-host.ps1` is for dev builds only (absolute path).
  - Add an **Installer** section: the install dir, files, per-user scope, `UpgradeCode`, MajorUpgrade with no downgrade, the Start Menu shortcut, uninstall removing the NM keys and the `Run` value (Ruling 7) but never user data, and the artifact names.
  - Add an **Updates** section:
    - the endpoint and headers;
    - tag → version and the `is_newer` rule;
    - drafts and prereleases are ignored;
    - the asset name;
    - the 24 h due rule, re-checked hourly;
    - `autoUpdate` (default true) and `lastAutoUpdateCheck` in `settings.json`;
    - installed-copy-only (Ruling 3);
    - `DISABLE_AUTOUPDATER=1`;
    - the magic and size checks (Ruling 6);
    - the relauncher sequence (Ruling 5).
  - Add a **Release** section: `release-windows.yml`, its guardrails, and how it is enforced.
- [ ] **Step 2: `CLAUDE.md`.**
  - Build/Test Commands: add `powershell -File apps/windows/scripts/build-msi.ps1` (needs WiX 3.14 + cargo-wix) and `powershell -File apps/windows/scripts/test-pack-extension.ps1`.
  - Windows section: add an SP6 sentence (MSI, auto-update `updater.rs` + `core::update`, release workflow).
  - Releasing section: one line saying `release-windows.yml` attaches `ClaudeDashboard-x64.msi` and `claude-dashboard-extension.zip` after `release.sh` publishes, and that the manual fallback is `workflow_dispatch` with the tag.
- [ ] **Step 3: Full verification.**
  - `cd apps/linux && cargo test -p claude-dashboard-core && cargo clippy --workspace --all-targets -- -D warnings`
  - `cd apps/windows && cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo build -p claude-dashboard`
  - `cd apps/windows/extension && node --test`
  - `bash scripts/test-sync-version.sh`, `bash scripts/test-release-workflow.sh`, and `powershell -File apps/windows/scripts/test-pack-extension.ps1`

  Paste the summary lines.
- [ ] **Step 4: BOM check** over every file changed since the plan commit.
- [ ] **Step 5: Commit** with message `docs(windows): installer, updates and release contract` and the trailer.
