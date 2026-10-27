//! Detect and quit Claude Desktop (port of macOS `ClaudeDesktopProcess`). Desktop's process is
//! `claude.exe`, the same image name as Claude Code's CLI, so a process counts only when its full
//! image path is exactly an install's app executable; see `is_desktop_exe`.

/// True when `path` is the app executable of a Claude Desktop install:
/// `...\AnthropicClaude\app-<anything>\claude.exe` or `...\WindowsApps\Claude_<anything>\app\claude.exe`
/// (case-insensitive, the file directly in that folder). Electron helpers share the path and match;
/// data-dir binaries, `claude-code` folders, `~\.local\bin` and npm never do.
pub fn is_desktop_exe(path: &str) -> bool {
    let lower = path.to_lowercase().replace('/', "\\");
    let parts: Vec<&str> = lower.split(&['\\'][..]).collect();
    let n = parts.len();
    if n < 3 || parts[n - 1] != "claude.exe" {
        return false;
    }
    let (dir, parent) = (parts[n - 2], parts[n - 3]);
    if dir.starts_with("app-") && parent == "anthropicclaude" {
        return true;
    }
    n >= 4 && dir == "app" && parent.starts_with("claude_") && parts[n - 4] == "windowsapps"
}

/// Detects and quits Claude Desktop. Electron rewrites its login files on quit, so the switcher
/// writes only while `is_running` is false.
pub trait DesktopProcess {
    fn is_running(&self) -> bool;
    /// Asks Claude Desktop to quit, force-terminating after at most 5 s.
    /// Returns true when no Claude Desktop process remains.
    fn quit(&self) -> bool;
}

#[cfg(windows)]
pub use win::Win32DesktopProcess;

#[cfg(windows)]
mod win {
    use super::{is_desktop_exe, DesktopProcess};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, INVALID_HANDLE_VALUE, LPARAM};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    const POLL: Duration = Duration::from_millis(100);
    const GRACE: Duration = Duration::from_secs(5);

    pub struct Win32DesktopProcess;

    /// Full image path of `pid`, or None when it can't be opened or queried.
    fn image_path(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            if ok == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Pids of every process whose image is a Claude Desktop executable.
    fn desktop_pids() -> Vec<u32> {
        let mut pids = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return pids;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snap, &mut entry) != 0;
            while more {
                let name_len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
                // Cheap name filter first; the path check is what decides.
                if name.eq_ignore_ascii_case("claude.exe") {
                    if let Some(path) = image_path(entry.th32ProcessID) {
                        if is_desktop_exe(&path) {
                            pids.push(entry.th32ProcessID);
                        }
                    }
                }
                more = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        pids
    }

    struct CloseCtx {
        pids: Vec<u32>,
    }

    unsafe extern "system" fn close_if_desktop(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &*(lparam as *const CloseCtx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if ctx.pids.contains(&pid) && IsWindowVisible(hwnd) != 0 {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        1
    }

    fn wait_until_gone(timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !desktop_pids().is_empty() && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        desktop_pids().is_empty()
    }

    impl DesktopProcess for Win32DesktopProcess {
        fn is_running(&self) -> bool {
            !desktop_pids().is_empty()
        }

        fn quit(&self) -> bool {
            let pids = desktop_pids();
            if pids.is_empty() {
                return true;
            }
            let ctx = CloseCtx { pids };
            unsafe { EnumWindows(Some(close_if_desktop), &ctx as *const CloseCtx as LPARAM) };
            if wait_until_gone(GRACE) {
                return true;
            }
            for pid in desktop_pids() {
                unsafe {
                    let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
                    if !h.is_null() {
                        TerminateProcess(h, 1);
                        CloseHandle(h);
                    }
                }
            }
            wait_until_gone(Duration::from_secs(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_desktop_installs() {
        assert!(is_desktop_exe(r"C:\Users\x\AppData\Local\AnthropicClaude\app-0.14.10\claude.exe"));
        assert!(is_desktop_exe(
            r"C:\Program Files\WindowsApps\Claude_0.14.10.0_x64__pzs8sxrjxfjjc\app\claude.exe"
        ));
        assert!(is_desktop_exe(r"c:\users\X\appdata\local\ANTHROPICCLAUDE\App-0.14.10\CLAUDE.EXE"));
        assert!(is_desktop_exe(r"C:\Program Files\WINDOWSAPPS\claude_1_x64__p\APP\Claude.exe"));
    }

    #[test]
    fn never_matches_claude_code_or_other_binaries() {
        for p in [
            r"C:\Users\x\.local\bin\claude.exe",
            r"C:\Users\x\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\claude.exe",
            r"C:\Users\x\AppData\Roaming\Claude\claude-code\2.0.1\claude.exe",
            r"C:\Users\x\AppData\Local\AnthropicClaude\app-0.14.10\resources\claude.exe",
            r"C:\Users\x\AppData\Local\AnthropicClaude\Update.exe",
            r"C:\Program Files\WindowsApps\Claude_x\app\resources\claude.exe",
            r"claude.exe",
            "",
        ] {
            assert!(!is_desktop_exe(p), "{p}");
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_is_running_false_without_desktop() {
        assert!(!Win32DesktopProcess.is_running());
    }
}
