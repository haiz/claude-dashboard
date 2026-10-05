//! Reload pipe: `\\.\pipe\claude-dashboard`. A message `reload` (sent by the
//! native-messaging bridge, or by a second app instance) triggers a refresh.

pub const PIPE_NAME: &str = r"\\.\pipe\claude-dashboard";

/// A message on the pipe: `reload` refreshes, `show` also raises the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeMsg {
    Reload,
    Show,
}

pub fn parse(msg: &str) -> Option<PipeMsg> {
    match msg.trim() {
        "reload" => Some(PipeMsg::Reload),
        "show" => Some(PipeMsg::Show),
        _ => None,
    }
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

/// Spawns a daemon thread serving the pipe; calls `on_msg` per recognised message.
#[cfg(windows)]
pub fn serve<F: Fn(PipeMsg) + Send + 'static>(on_msg: F) {
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
            if ok != 0 {
                if let Some(m) = parse(&String::from_utf8_lossy(&buf[..n as usize])) {
                    on_msg(m);
                }
            }
            unsafe {
                DisconnectNamedPipe(h);
                CloseHandle(h);
            }
        }
    });
}

#[cfg(not(windows))]
pub fn serve<F: Fn(PipeMsg) + Send + 'static>(_on_msg: F) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messages() {
        assert_eq!(parse("reload"), Some(PipeMsg::Reload));
        assert_eq!(parse("reload\n"), Some(PipeMsg::Reload));
        assert_eq!(parse("show"), Some(PipeMsg::Show));
        assert_eq!(parse(" show "), Some(PipeMsg::Show));
        assert_eq!(parse("quit"), None);
        assert_eq!(parse(""), None);
    }
}
