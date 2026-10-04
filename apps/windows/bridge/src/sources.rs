//! `extension-sources.json` handling now lives in
//! `claude_dashboard_core::extension_sources` (shared with the app); re-exported
//! here so the bridge's handler and real env are unchanged.

pub use claude_dashboard_core::extension_sources::*;
