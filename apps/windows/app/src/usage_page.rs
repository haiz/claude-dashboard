//! The account page's "Open Usage Page" (macOS `UsagePage`): opens claude.ai's usage page in
//! the browser the account signs in with, so it loads as that account. A scanned account opens
//! its own profile, an extension-fed account its browser (no profile switch, as the Switch
//! sign-in does), and a pasted key, or a browser not installed in a known place, the default
//! browser.
//!
//!     usage_page::open(&account, &extension_sources);

use std::path::PathBuf;

use claude_dashboard_core::account_kind::{kind_of, AccountKind};
use claude_dashboard_core::extension_sources::ExtensionSources;
use claude_dashboard_core::model::{Account, AccountSource, Browser};

use crate::cc_login::{browser_command, install_roots};

pub const URL: &str = "https://claude.ai/settings/usage";

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// `browser`, in profile `profile_dir` when given.
    Browser { browser: Browser, profile_dir: Option<String> },
    Default,
}

pub fn target(a: &Account, sources: &ExtensionSources) -> Target {
    match kind_of(a, sources) {
        AccountKind::Scanned { browser, .. } if a.source == AccountSource::Browser && !a.chrome_profile_path.is_empty() => {
            Target::Browser { browser, profile_dir: Some(a.chrome_profile_path.clone()) }
        }
        AccountKind::Extension { browser } => Target::Browser { browser, profile_dir: None },
        _ => Target::Default,
    }
}

/// The exe and arguments that open the usage page for `target`, looking for the browser under
/// `roots`; the default browser (`rundll32 url.dll,FileProtocolHandler`) when it is not there.
pub fn command(target: &Target, roots: &[PathBuf]) -> (PathBuf, Vec<String>) {
    if let Target::Browser { browser, profile_dir } = target {
        if let Some(cmd) = browser_command(browser, profile_dir.as_deref(), URL, roots) {
            return cmd;
        }
    }
    (PathBuf::from("rundll32.exe"), vec!["url.dll,FileProtocolHandler".to_string(), URL.to_string()])
}

/// Launches the browser; false when the process could not start.
pub fn open(a: &Account, sources: &ExtensionSources) -> bool {
    let (exe, args) = command(&target(a, sources), &install_roots());
    std::process::Command::new(exe).args(args).spawn().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::extension_sources::Binding;

    fn account(json: &str) -> Account {
        Account::from_json_object(json).unwrap()
    }

    fn scanned() -> Account {
        account(r#"{"id":"s","name":"s","chromeProfilePath":"Profile 2","browser":"edge","plan":"Pro","status":"active"}"#)
    }

    fn pasted() -> Account {
        account(r#"{"id":"p","name":"p","chromeProfilePath":"","plan":"Pro","status":"active","source":"manual"}"#)
    }

    fn default_command() -> (PathBuf, Vec<String>) {
        (PathBuf::from("rundll32.exe"), vec!["url.dll,FileProtocolHandler".into(), URL.into()])
    }

    #[test]
    fn url_is_the_usage_settings_page() {
        assert_eq!(URL, "https://claude.ai/settings/usage");
    }

    #[test]
    fn target_per_account_kind() {
        let none = ExtensionSources::default();
        assert_eq!(target(&scanned(), &none), Target::Browser { browser: Browser::Edge, profile_dir: Some("Profile 2".into()) });
        assert_eq!(target(&pasted(), &none), Target::Default);

        let mut sources = ExtensionSources::default();
        sources.bindings.insert("install".into(), Binding { account_id: "p".into(), browser: "brave".into() });
        assert_eq!(target(&pasted(), &sources), Target::Browser { browser: Browser::Brave, profile_dir: None });
    }

    #[test]
    fn command_opens_the_profile_when_the_browser_is_installed() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("Microsoft").join("Edge").join("Application").join("msedge.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        let t = target(&scanned(), &ExtensionSources::default());
        assert_eq!(command(&t, &[d.path().to_path_buf()]), (exe, vec!["--profile-directory=Profile 2".into(), URL.into()]));
    }

    #[test]
    fn command_falls_back_to_the_default_browser() {
        let d = tempfile::tempdir().unwrap();
        let roots = [d.path().to_path_buf()];
        // Browser not installed under the roots.
        assert_eq!(command(&target(&scanned(), &ExtensionSources::default()), &roots), default_command());
        // Arc has no Windows command.
        assert_eq!(command(&Target::Browser { browser: Browser::Arc, profile_dir: None }, &roots), default_command());
        // A pasted key.
        assert_eq!(command(&Target::Default, &roots), default_command());
    }
}
