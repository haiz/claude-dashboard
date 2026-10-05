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

#[allow(dead_code)] // used by main.rs (Task 4)
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

#[allow(dead_code)] // used by main.rs (Task 4)
pub fn install_dir(local_appdata: &Path) -> PathBuf {
    local_appdata.join("Programs").join("ClaudeDashboard")
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

#[allow(dead_code)] // used by main.rs (Task 4)
pub fn is_installed_copy(exe: &Path, local_appdata: &Path) -> bool {
    let want = install_dir(local_appdata).join("claude-dashboard.exe");
    norm(exe) == norm(&want)
}

#[allow(dead_code)] // used by main.rs (Task 4)
pub fn auto_update_allowed(enabled: bool, installed: bool, disable_env: Option<&str>) -> bool {
    enabled && installed && disable_env != Some("1")
}

pub fn looks_like_msi(head: &[u8]) -> bool {
    head.len() >= MSI_MAGIC.len() && head[..MSI_MAGIC.len()] == MSI_MAGIC
}

/// PowerShell single-quoted literal, with embedded `'` doubled.
pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn relaunch_script(pid: u32, msi: &Path, exe: &Path) -> String {
    // msiexec gets its argument string unquoted from PowerShell 5.1, so the
    // MSI path carries its own double quotes.
    let msi_arg = ps_quote(&format!("\"{}\"", msi.display()));
    let exe_arg = ps_quote(&exe.display().to_string());
    [
        "$ErrorActionPreference = 'SilentlyContinue'".to_string(),
        format!("Wait-Process -Id {pid} -Timeout 30"),
        format!(
            "Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', {msi_arg}, '/passive', '/norestart') -Wait"
        ),
        format!("Start-Process -FilePath {exe_arg}"),
    ]
    .join("; ")
}

#[allow(dead_code)] // used by main.rs (Task 4)
pub fn download(info: &UpdateInfo, current_version: &str) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!(
        "ClaudeDashboard-update-{}-{}.msi",
        info.version,
        uuid::Uuid::new_v4()
    ));
    let result = download_to(info, current_version, &path);
    if result.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    result.map(|()| path)
}

fn download_to(info: &UpdateInfo, current_version: &str, path: &Path) -> Result<(), String> {
    let resp = ureq::get(&info.download_url)
        .set("User-Agent", &format!("claude-dashboard/{current_version}"))
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut file = std::fs::File::create(path).map_err(|e| format!("Download failed: {e}"))?;
    let copied = std::io::copy(&mut resp.into_reader().take(MAX_MSI_BYTES + 1), &mut file)
        .map_err(|e| format!("Download failed: {e}"))?;
    file.flush().map_err(|e| format!("Download failed: {e}"))?;
    drop(file);
    if copied > MAX_MSI_BYTES {
        return Err("The update is too large.".into());
    }
    let mut head = [0u8; 8];
    let n = std::fs::File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| format!("Download failed: {e}"))?;
    if !looks_like_msi(&head[..n]) {
        return Err("The downloaded update is not a Windows installer.".into());
    }
    Ok(())
}

/// Spawns the relauncher detached and hidden; the caller then quits.
#[allow(dead_code)] // used by main.rs (Task 4)
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

#[allow(dead_code)] // used by main.rs (Task 4)
pub fn check(current_version: &str) -> Result<Option<UpdateInfo>, String> {
    let json = fetch_latest_release(current_version).map_err(|e| e.to_string())?;
    parse_release(&json, current_version, MSI_ASSET).map_err(|e| e.to_string())
}

#[allow(dead_code)] // used by main.rs (Task 4)
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
