//! Update guards, MSI download and the detached relauncher.
//! Auto-install only ever runs from the installed copy; see `is_installed_copy`.

use claude_dashboard_core::update::{
    fetch_latest_release, parse_release, UpdateInfo, MSI_ASSET,
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// OLE compound-file header, which every MSI starts with.
pub const MSI_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
pub const MAX_MSI_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Downloading,
    Installing,
    DevBuild(Option<UpdateInfo>),
    Failed(String),
}

pub fn install_dir(local_appdata: &Path) -> PathBuf {
    local_appdata.join("Programs").join("ClaudeDashboard")
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

pub fn is_installed_copy(exe: &Path, local_appdata: &Path) -> bool {
    let want = install_dir(local_appdata).join("claude-dashboard.exe");
    norm(exe) == norm(&want)
}

pub fn auto_update_allowed(enabled: bool, installed: bool, disable_env: Option<&str>) -> bool {
    enabled && installed && disable_env != Some("1")
}

pub fn looks_like_msi(head: &[u8]) -> bool {
    head.len() >= MSI_MAGIC.len() && head[..MSI_MAGIC.len()] == MSI_MAGIC
}

/// PowerShell single-quoted literal. PowerShell treats U+2018/2019/201A/201B
/// as single quotes too, so every one of them is doubled.
pub fn ps_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        out.push(c);
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// The release tag is remote input; only allow `[0-9A-Za-z.-]+`.
pub fn valid_version(v: &str) -> bool {
    !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

pub fn relaunch_script(pid: u32, msi: &Path, exe: &Path) -> String {
    // msiexec gets its argument string unquoted from PowerShell 5.1, so the
    // MSI path carries its own double quotes.
    let msi_arg = ps_quote(&format!("\"{}\"", msi.display()));
    let exe_arg = ps_quote(&exe.display().to_string());
    let msi_plain = ps_quote(&msi.display().to_string());
    [
        "$ErrorActionPreference = 'SilentlyContinue'".to_string(),
        format!("Wait-Process -Id {pid} -Timeout 30"),
        format!(
            "Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', {msi_arg}, '/passive', '/norestart') -Wait"
        ),
        format!("Remove-Item -LiteralPath {msi_plain} -Force"),
        format!("Start-Process -FilePath {exe_arg}"),
    ]
    .join("; ")
}

pub fn download(info: &UpdateInfo, current_version: &str) -> Result<PathBuf, String> {
    if !valid_version(&info.version) {
        return Err("The release version is not valid.".into());
    }
    let path = std::env::temp_dir().join(format!(
        "ClaudeDashboard-update-{}-{}.msi",
        info.version,
        uuid::Uuid::new_v4()
    ));
    let resp = ureq::get(&info.download_url)
        .set("User-Agent", &format!("claude-dashboard/{current_version}"))
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    write_capped(resp.into_reader(), &path)?;
    Ok(path)
}

/// Streams `reader` into `path` with the size cap and MSI magic check;
/// the file is deleted on any failure.
fn write_capped(reader: impl Read, path: &Path) -> Result<(), String> {
    write_capped_with(reader, path, MAX_MSI_BYTES)
}

fn write_capped_with(reader: impl Read, path: &Path, max: u64) -> Result<(), String> {
    let result = write_capped_inner(reader, path, max);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

fn write_capped_inner(reader: impl Read, path: &Path, max: u64) -> Result<(), String> {
    let io_err = |e: std::io::Error| format!("Download failed: {e}");
    let mut file = std::fs::File::create(path).map_err(io_err)?;
    let copied = std::io::copy(&mut reader.take(max + 1), &mut file).map_err(io_err)?;
    file.flush().map_err(io_err)?;
    drop(file);
    if copied > max {
        return Err("The update is too large.".into());
    }
    let mut head = [0u8; 8];
    let magic = std::fs::File::open(path)
        .map_err(io_err)
        .and_then(|mut f| match f.read_exact(&mut head) {
            Ok(()) => Ok(looks_like_msi(&head)),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
            Err(e) => Err(io_err(e)),
        })?;
    if !magic {
        return Err("The downloaded update is not a Windows installer.".into());
    }
    Ok(())
}

/// Spawns the relauncher detached and hidden; the caller then quits.
pub fn apply(msi: &Path, exe: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let ps = Path::new(&root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let script = relaunch_script(std::process::id(), msi, exe);
    Command::new(ps)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
        ])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start the installer: {e}"))
}

pub fn check(current_version: &str) -> Result<Option<UpdateInfo>, String> {
    let json = fetch_latest_release(current_version).map_err(|e| e.to_string())?;
    parse_release(&json, current_version, MSI_ASSET).map_err(|e| e.to_string())
}

/// When the check fails because the release's MSI isn't uploaded yet, the
/// `last_auto_update_check_unix` stamp to write so the next hourly tick retries.
pub fn restamp_after(err_msg: &str, now: f64) -> Option<f64> {
    use claude_dashboard_core::update::{UpdateError, CHECK_INTERVAL_S};
    (err_msg == UpdateError::AssetNotFound.to_string()).then_some(now - CHECK_INTERVAL_S + 3600.0)
}

pub fn status_text(state: &UpdateState, current: &str) -> String {
    match state {
        UpdateState::Idle => format!("Version {current}"),
        UpdateState::Checking => "Checking for updates…".into(),
        UpdateState::UpToDate => format!("Up to date (version {current})"),
        UpdateState::Available(i) => format!("Version {} is available", i.version),
        UpdateState::Downloading => "Downloading update…".into(),
        UpdateState::Installing => "Installing update…".into(),
        UpdateState::DevBuild(None) => "Development build — install the MSI to update".into(),
        UpdateState::DevBuild(Some(i)) => {
            format!("Version {} is available — install the MSI to update", i.version)
        }
        UpdateState::Failed(m) => m.clone(),
    }
}

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
        assert_eq!(ps_quote("O\u{2019}Brien"), "'O\u{2019}\u{2019}Brien'");
        for q in ['\u{2018}', '\u{201A}', '\u{201B}'] {
            assert_eq!(ps_quote(&format!("a{q}b")), format!("'a{q}{q}b'"));
        }
    }

    #[test]
    fn relaunch_script_doubles_unicode_quotes() {
        let s = relaunch_script(
            1,
            Path::new("C:\\Users\\O\u{2019}B\\x.msi"),
            Path::new("C:\\Users\\O\u{2019}B\\claude-dashboard.exe"),
        );
        assert!(s.contains("'C:\\Users\\O\u{2019}\u{2019}B\\claude-dashboard.exe'"), "{s}");
        assert!(s.contains("\"C:\\Users\\O\u{2019}\u{2019}B\\x.msi\""), "{s}");
    }

    #[test]
    fn relaunch_script_deletes_msi_between_msiexec_and_relaunch() {
        let s = relaunch_script(
            7,
            Path::new(r"C:\Users\O'Brien\Temp\u.msi"),
            Path::new(r"C:\Users\O'Brien\claude-dashboard.exe"),
        );
        let rm = s.find(r"Remove-Item -LiteralPath 'C:\Users\O''Brien\Temp\u.msi' -Force").expect(&s);
        assert!(s.find("msiexec").unwrap() < rm, "{s}");
        assert!(rm < s.rfind("Start-Process").unwrap(), "{s}");
    }

    #[test]
    fn restamp_only_for_missing_asset() {
        let missing = claude_dashboard_core::update::UpdateError::AssetNotFound.to_string();
        let now = 1_000_000.0;
        let want = now - claude_dashboard_core::update::CHECK_INTERVAL_S + 3600.0;
        assert_eq!(restamp_after(&missing, now), Some(want));
        assert_eq!(restamp_after("network down", now), None);
    }

    /// Manual end-to-end check: `CD_E2E_MSI=... CD_E2E_EXE=... cargo test -- --ignored e2e_apply_relaunch`.
    /// Returning lets the test process exit so the relauncher proceeds.
    #[test]
    #[ignore]
    fn e2e_apply_relaunch() {
        let (Ok(msi), Ok(exe)) = (std::env::var("CD_E2E_MSI"), std::env::var("CD_E2E_EXE")) else {
            eprintln!("CD_E2E_MSI / CD_E2E_EXE not set; skipping");
            return;
        };
        assert_eq!(apply(Path::new(&msi), Path::new(&exe)), Ok(()));
    }

    #[test]
    fn version_validation() {
        assert!(valid_version("1.19.0"));
        assert!(valid_version("1.19.0-rc1"));
        for bad in ["", "1.0\u{2019};calc", "1/../x", "1 0"] {
            assert!(!valid_version(bad), "{bad}");
        }
    }

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("u.msi");
        (d, p)
    }

    #[test]
    fn write_capped_rejects_oversize_and_deletes() {
        let (_d, p) = tmp();
        let mut body = MSI_MAGIC.to_vec();
        body.extend_from_slice(&[0u8; 20]);
        let r = write_capped_with(std::io::Cursor::new(body), &p, 16);
        assert_eq!(r, Err("The update is too large.".to_string()));
        assert!(!p.exists());
        let r = write_capped_with(std::io::repeat(0xD0), &p, 16);
        assert!(r.is_err());
        assert!(!p.exists());
    }

    #[test]
    fn write_capped_rejects_html_and_short_and_deletes() {
        let (_d, p) = tmp();
        let want = Err("The downloaded update is not a Windows installer.".to_string());
        assert_eq!(write_capped_with(std::io::Cursor::new(b"<!DOCTYPE html><html>".to_vec()), &p, 1024), want);
        assert!(!p.exists());
        assert_eq!(write_capped_with(std::io::Cursor::new(MSI_MAGIC[..4].to_vec()), &p, 1024), want);
        assert!(!p.exists());
    }

    #[test]
    fn write_capped_accepts_msi_within_and_at_cap() {
        let (_d, p) = tmp();
        let mut body = MSI_MAGIC.to_vec();
        body.extend_from_slice(&[7u8; 4]);
        assert_eq!(write_capped_with(std::io::Cursor::new(body.clone()), &p, 1024), Ok(()));
        assert!(p.exists());
        let _ = std::fs::remove_file(&p);
        assert_eq!(write_capped_with(std::io::Cursor::new(body), &p, 12), Ok(()), "exactly at cap");
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 12);
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
