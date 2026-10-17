//! The Switch flow's rules and texts (macOS `DashboardViewModel` Claude Code Switch section):
//! when to ask first, what to say, and what a first login needs for an account.

use claude_dashboard_core::account_kind::{kind_of, AccountKind};
use claude_dashboard_core::extension_sources::ExtensionSources;
use claude_dashboard_core::model::{Account, AccountSource};

use crate::cc_login::ProvisionInput;

pub const CONFIRM_TITLE: &str = "Switch Claude Code?";
pub const MESSAGE_TITLE: &str = "Claude Code";
pub const WAITING_TITLE: &str = "Finish signing in";
pub const WAITING_TEXT: &str = "Claude Dashboard opened your browser profile for this account. Sign in and click Authorize to finish, then it switches automatically.";

/// The browser sign-in takes focus and the flyout hides on focus loss, taking its prompt
/// with it: when neither window is showing, a prompt opens the main window.
pub fn needs_main_window(flyout_visible: bool, main_visible: bool) -> bool {
    !flyout_visible && !main_visible
}

/// True while the active account still has quota left (5h under 95% and 7d under 99%): a
/// switch then asks first (macOS `hasQuotaLeft`).
pub fn needs_confirm(active_usage: Option<(f64, f64)>) -> bool {
    active_usage.is_some_and(|(five, seven)| five < 95.0 && seven < 99.0)
}

pub fn confirm_message(active_name: &str, five: f64, seven: f64, target_name: &str) -> String {
    format!(
        "{active_name} still has quota left (5h {}%, 7d {}%). Switch Claude Code to {target_name} anyway?",
        five as i64, seven as i64
    )
}

pub fn success_message(name: &str) -> String {
    format!("Claude Code now uses {name}. Running sessions pick it up within about 30 seconds.")
}

pub fn needs_login_message(name: &str) -> String {
    format!("Claude Code lost the login for {name}. Run /login once with it.")
}

pub fn resync_first_message(name: &str) -> String {
    format!("Re-sync {name} first so the dashboard knows its browser profile.")
}

pub fn store_failed_message(name: &str) -> String {
    format!("Could not save the new Claude Code login for {name}.")
}

/// What the first login needs for `a`: its email and browser, with the profile directory of a
/// scanned account; an extension-fed account opens its browser without a profile. `None` for
/// a pasted key (no browser behind it) or an account without an email.
pub fn provision_input(a: &Account, sources: &ExtensionSources) -> Option<ProvisionInput> {
    let email = a.email.clone()?;
    match kind_of(a, sources) {
        AccountKind::Scanned { browser, .. } if a.source == AccountSource::Browser && !a.chrome_profile_path.is_empty() => {
            Some(ProvisionInput { email, browser, profile_dir: Some(a.chrome_profile_path.clone()) })
        }
        AccountKind::Extension { browser } => Some(ProvisionInput { email, browser, profile_dir: None }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_opens_main_window_only_when_nothing_shows() {
        assert!(needs_main_window(false, false));
        assert!(!needs_main_window(true, false));
        assert!(!needs_main_window(false, true));
    }

    #[test]
    fn confirm_only_with_quota_left() {
        assert!(needs_confirm(Some((94.0, 98.0))));
        assert!(!needs_confirm(Some((95.0, 10.0))));
        assert!(!needs_confirm(Some((10.0, 99.0))));
        assert!(!needs_confirm(None));
    }

    #[test]
    fn confirm_message_matches_macos() {
        assert_eq!(
            confirm_message("a@x.com", 42.7, 80.2, "b@x.com"),
            "a@x.com still has quota left (5h 42%, 7d 80%). Switch Claude Code to b@x.com anyway?"
        );
    }

    #[test]
    fn success_message_matches_macos() {
        assert_eq!(
            success_message("b@x.com"),
            "Claude Code now uses b@x.com. Running sessions pick it up within about 30 seconds."
        );
        assert_eq!(needs_login_message("a@x.com"), "Claude Code lost the login for a@x.com. Run /login once with it.");
        assert_eq!(resync_first_message("a@x.com"), "Re-sync a@x.com first so the dashboard knows its browser profile.");
        assert_eq!(store_failed_message("a@x.com"), "Could not save the new Claude Code login for a@x.com.");
    }

    fn account(json: &str) -> Account {
        Account::from_json_object(json).unwrap()
    }

    #[test]
    fn provision_input_per_account_kind() {
        let scanned = account(r#"{"id":"s","name":"s","email":"s@x.com","chromeProfilePath":"Profile 2","browser":"edge","plan":"Pro","status":"active"}"#);
        let i = provision_input(&scanned, &ExtensionSources::default()).unwrap();
        assert_eq!((i.email.as_str(), i.profile_dir.as_deref()), ("s@x.com", Some("Profile 2")));
        assert_eq!(i.browser, claude_dashboard_core::model::Browser::Edge);

        let pasted = account(r#"{"id":"p","name":"p","email":"p@x.com","chromeProfilePath":"","plan":"Pro","status":"active","source":"manual"}"#);
        assert!(provision_input(&pasted, &ExtensionSources::default()).is_none());

        let mut sources = ExtensionSources::default();
        sources.bindings.insert(
            "install".into(),
            claude_dashboard_core::extension_sources::Binding { account_id: "p".into(), browser: "edge".into() },
        );
        let ext = provision_input(&pasted, &sources).unwrap();
        assert_eq!(ext.profile_dir, None);
        assert_eq!(ext.browser, claude_dashboard_core::model::Browser::Edge);

        let no_email = account(r#"{"id":"n","name":"n","chromeProfilePath":"Default","plan":"Pro","status":"active"}"#);
        assert!(provision_input(&no_email, &ExtensionSources::default()).is_none());
    }
}
