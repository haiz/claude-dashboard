//! Claude Desktop account switch on Windows (port of macOS `Services/ClaudeDesktopSwitch`):
//! Chromium safe-storage sealing and where Claude Desktop keeps its data.

pub mod safe_storage;
pub mod paths;
pub mod config;
pub mod cookies;
pub mod process;
