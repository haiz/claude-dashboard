//! The app's Claude Code switcher and what each account's Switch button can do (macOS
//! `DashboardViewModel.refreshClaudeCodeSwitchState` / `switchAvailability`).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use claude_dashboard_core::cc_switch::switcher::{Availability, SwitchAccount, Switcher};
use claude_dashboard_core::model::Account;

static SWITCHER: OnceLock<Option<Switcher>> = OnceLock::new();
static AVAILABILITY: Mutex<Option<HashMap<String, Availability>>> = Mutex::new(None);

/// The switcher for this run: `None` in dev runs, which must never touch Claude Code's files
/// (macOS disables it under XCTest the same way).
pub fn make(enabled: bool) -> Option<Switcher> {
    enabled.then(Switcher::live)
}

/// Call once at startup.
pub fn init(enabled: bool) {
    let _ = SWITCHER.set(make(enabled));
}

pub fn switcher() -> Option<&'static Switcher> {
    SWITCHER.get().and_then(Option::as_ref)
}

pub fn switch_accounts(accounts: &[Account]) -> Vec<SwitchAccount> {
    accounts.iter().map(|a| SwitchAccount { id: a.id.clone(), email: a.email.clone() }).collect()
}

/// Copies the active Claude Code login into the vault, then recomputes every account's
/// availability. Runs on the refresh thread (file and DPAPI I/O).
pub fn refresh(accounts: &[Account]) {
    let Some(s) = switcher() else { return };
    let all = switch_accounts(accounts);
    if let Err(e) = s.capture(&all) {
        eprintln!("[cc-switch] capture failed: {e:?}");
    }
    *AVAILABILITY.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.availability(&all));
}

pub fn availability(id: &str) -> Option<Availability> {
    AVAILABILITY.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(id).copied())
}

/// `(switch-state, switch-help)` for a row: `""` hides the button (no switcher, or no state
/// yet), `active` hides it too (the badge marks that account).
pub fn switch_fields(a: Option<Availability>) -> (String, String) {
    let Some(a) = a else { return (String::new(), String::new()) };
    let state = match a {
        Availability::Active => "active",
        Availability::Ready => "ready",
        Availability::NotCaptured => "not-captured",
        Availability::LoginExpired => "login-expired",
        Availability::NeedsLogin => "needs-login",
    };
    (state.into(), a.help().unwrap_or_default().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_maps_to_row_fields() {
        assert_eq!(switch_fields(None), (String::new(), String::new()));
        assert_eq!(switch_fields(Some(Availability::Active)), ("active".into(), String::new()));
        assert_eq!(switch_fields(Some(Availability::Ready)), ("ready".into(), "Use this account in Claude Code".into()));
        assert_eq!(switch_fields(Some(Availability::NotCaptured)).0, "not-captured");
        assert_eq!(switch_fields(Some(Availability::LoginExpired)).0, "login-expired");
        assert_eq!(
            switch_fields(Some(Availability::NeedsLogin)),
            ("needs-login".into(), "Claude Code lost this login. Run /login with this account".into())
        );
    }

    #[test]
    fn dev_run_has_no_switcher() {
        assert!(make(false).is_none());
        assert!(make(true).is_some());
    }
}
