//! Tells a running app to reload the store, via the named pipe it listens on.
//! A missing pipe means the app is not running; that is not an error. The
//! app's pipe-server side is built in sub-project 2; the bridge only writes.

/// The pipe the app serves. `reload` is the only message.
pub const PIPE_NAME: &str = r"\\.\pipe\claude-dashboard";

#[cfg(windows)]
pub fn reload() {
    use std::fs::OpenOptions;
    use std::io::Write;
    if let Ok(mut pipe) = OpenOptions::new().write(true).open(PIPE_NAME) {
        let _ = pipe.write_all(b"reload\n");
    }
}

#[cfg(not(windows))]
pub fn reload() {
    // No named pipe off Windows; the host runs on Windows in production. This
    // keeps the crate building and testable on Linux/macOS CI.
}
