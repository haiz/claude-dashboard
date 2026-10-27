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
    use windows_sys::Win32::Foundation::{
        CloseHandle, BOOL, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        PROCESS_TERMINATE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    const POLL: Duration = Duration::from_millis(100);
    const GRACE: Duration = Duration::from_secs(5);
    const FORCE_WAIT: Duration = Duration::from_secs(1);
    /// Enough to wait on and to force-quit.
    const QUIT_ACCESS: PROCESS_ACCESS_RIGHTS =
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;

    pub struct Win32DesktopProcess;

    /// An open handle to a process whose image was checked through that same handle. While it
    /// is open the pid cannot be reused, so WM_CLOSE and TerminateProcess only ever reach the
    /// Claude Desktop process that was checked, never a later process (e.g. Claude Code's
    /// `claude.exe`) that got the same pid.
    struct Held {
        pid: u32,
        handle: HANDLE,
    }

    impl Held {
        fn exited(&self) -> bool {
            unsafe { WaitForSingleObject(self.handle, 0) == WAIT_OBJECT_0 }
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }

    /// Full image path of the process behind `h`, or None when it can't be queried.
    fn image_path(h: HANDLE) -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) };
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }

    /// Opens `pid` with the first access mask that succeeds and keeps the handle only when its
    /// image is a Claude Desktop executable.
    fn open_desktop(pid: u32, access: &[PROCESS_ACCESS_RIGHTS]) -> Option<Held> {
        let handle = access.iter().find_map(|&a| {
            let h = unsafe { OpenProcess(a, 0, pid) };
            (!h.is_null()).then_some(h)
        })?;
        let held = Held { pid, handle };
        is_desktop_exe(&image_path(held.handle)?).then_some(held)
    }

    /// Every running Claude Desktop process, each opened with the first mask in `access` that
    /// succeeds.
    fn desktop_processes(access: &[PROCESS_ACCESS_RIGHTS]) -> Vec<Held> {
        let mut held = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return held;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snap, &mut entry) != 0;
            while more {
                let name_len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
                // Cheap name filter first; the path check is what decides.
                if name.eq_ignore_ascii_case("claude.exe") {
                    if let Some(h) = open_desktop(entry.th32ProcessID, access) {
                        held.push(h);
                    }
                }
                more = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        held
    }

    fn any_desktop() -> bool {
        !desktop_processes(&[PROCESS_QUERY_LIMITED_INFORMATION]).is_empty()
    }

    /// Opens for quitting; falls back to a wait-only handle so a process that refuses
    /// PROCESS_TERMINATE still gets WM_CLOSE and is waited on.
    fn desktop_processes_for_quit() -> Vec<Held> {
        desktop_processes(&[QUIT_ACCESS, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE])
    }

    struct CloseCtx {
        pids: Vec<u32>,
    }

    /// Posts WM_CLOSE to visible windows of held pids only.
    unsafe extern "system" fn close_if_desktop(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &*(lparam as *const CloseCtx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != 0 && ctx.pids.contains(&pid) && IsWindowVisible(hwnd) != 0 {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        1
    }

    /// Polls the held handles until every process has exited or `timeout` passes.
    fn wait_held(held: &[Held], timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if held.iter().all(Held::exited) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL);
        }
    }

    impl DesktopProcess for Win32DesktopProcess {
        fn is_running(&self) -> bool {
            any_desktop()
        }

        fn quit(&self) -> bool {
            let mut held = desktop_processes_for_quit();
            if held.is_empty() {
                return true;
            }
            let ctx = CloseCtx { pids: held.iter().map(|h| h.pid).collect() };
            unsafe { EnumWindows(Some(close_if_desktop), &ctx as *const CloseCtx as LPARAM) };
            if !wait_held(&held, GRACE) {
                // Helpers Desktop spawned during the grace period are forced too.
                for h in desktop_processes_for_quit() {
                    if !held.iter().any(|o| o.pid == h.pid) {
                        held.push(h);
                    }
                }
                for h in held.iter().filter(|h| !h.exited()) {
                    unsafe { TerminateProcess(h.handle, 1) };
                }
                wait_held(&held, FORCE_WAIT);
            }
            drop(held);
            // A fresh look: a Desktop launched meanwhile still counts as remaining.
            !any_desktop()
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
