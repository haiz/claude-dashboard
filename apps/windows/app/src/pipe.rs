//! Reload pipe: `\\.\pipe\claude-dashboard`. A message `reload` (sent by the
//! native-messaging bridge, or by a second app instance) triggers a refresh.

pub const PIPE_NAME: &str = r"\\.\pipe\claude-dashboard";

/// True for a message that asks for an immediate refresh.
pub fn is_reload(msg: &str) -> bool {
    matches!(msg.trim(), "reload" | "show")
}

/// Best-effort: send one message to the running instance's pipe.
#[cfg(windows)]
pub fn send(msg: &str) -> bool {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .open(PIPE_NAME)
        .and_then(|mut f| f.write_all(msg.as_bytes()))
        .is_ok()
}

#[cfg(not(windows))]
pub fn send(_msg: &str) -> bool {
    false
}

/// Spawns a daemon thread serving the pipe; calls `on_reload` per reload message.
#[cfg(windows)]
pub fn serve_reload<F: Fn() + Send + 'static>(on_reload: F) {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, PIPE_ACCESS_INBOUND};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    std::thread::spawn(move || {
        let name: Vec<u16> = PIPE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        loop {
            let h = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_INBOUND,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                    PIPE_UNLIMITED_INSTANCES,
                    512,
                    512,
                    0,
                    std::ptr::null(),
                )
            };
            if h == INVALID_HANDLE_VALUE {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
            // Returns 0 with ERROR_PIPE_CONNECTED if a client raced in first;
            // either way a client is attached, so read.
            unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            let mut buf = [0u8; 256];
            let mut n: u32 = 0;
            let ok = unsafe {
                ReadFile(h, buf.as_mut_ptr().cast(), buf.len() as u32, &mut n, std::ptr::null_mut())
            };
            if ok != 0 && is_reload(&String::from_utf8_lossy(&buf[..n as usize])) {
                on_reload();
            }
            unsafe {
                DisconnectNamedPipe(h);
                CloseHandle(h);
            }
        }
    });
}

#[cfg(not(windows))]
pub fn serve_reload<F: Fn() + Send + 'static>(_on_reload: F) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_reload_messages() {
        assert!(is_reload("reload"));
        assert!(is_reload("reload\n"));
        assert!(is_reload(" show "));
        assert!(!is_reload("quit"));
        assert!(!is_reload(""));
    }
}
