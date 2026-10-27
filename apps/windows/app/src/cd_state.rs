//! The app's Claude Desktop switcher and what each account's Desktop Switch can do (macOS
//! `DashboardViewModel` Claude Desktop sections). Modelled on `cc_state`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use claude_dashboard_core::cd_switch::switcher::{DesktopAccount, DesktopAvailability, DesktopSwitcher};
use claude_dashboard_core::model::Account;

static SWITCHER: OnceLock<Option<DesktopSwitcher>> = OnceLock::new();
static AVAILABILITY: Mutex<Option<HashMap<String, DesktopAvailability>>> = Mutex::new(None);
static DIFFERS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The switcher for this run: `None` in dev runs, which must never touch Claude Desktop's
/// files, and when Desktop is not installed.
pub fn make(enabled: bool) -> Option<DesktopSwitcher> {
    #[cfg(windows)]
    if enabled {
        return DesktopSwitcher::live();
    }
    let _ = enabled;
    None
}

/// Call once at startup.
pub fn init(enabled: bool) {
    let _ = SWITCHER.set(make(enabled));
}

pub fn switcher() -> Option<&'static DesktopSwitcher> {
    SWITCHER.get().and_then(Option::as_ref)
}

/// The accounts as the switcher sees them, session keys decrypted.
pub fn desktop_accounts(accounts: &[Account]) -> Vec<DesktopAccount> {
    accounts
        .iter()
        .map(|a| DesktopAccount {
            id: a.id.clone(),
            email: a.email.clone(),
            account_uuid: a.account_uuid.clone(),
            org_id: a.org_id.clone(),
            session_key: a.session_key.as_deref().and_then(claude_dashboard_core::store::decrypt_session_key),
        })
        .collect()
}

/// Desktop and Claude Code are signed in as different accounts (both known).
fn differs_rule(desktop: Option<&str>, code: Option<&str>) -> bool {
    matches!((desktop, code), (Some(d), Some(c)) if d != c)
}

/// Copies the active Claude Desktop login into the vault, then recomputes every account's
/// availability and whether Desktop differs from Claude Code. Runs on the refresh thread.
pub fn refresh(accounts: &[Account], cc_active_uuid: Option<&str>) {
    let Some(s) = switcher() else { return };
    let all = desktop_accounts(accounts);
    if let Err(e) = s.capture_active(&all) {
        eprintln!("[cd-switch] capture failed: {e}");
    }
    DIFFERS.store(
        differs_rule(s.active_account_uuid().as_deref(), cc_active_uuid),
        std::sync::atomic::Ordering::SeqCst,
    );
    *AVAILABILITY.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.availability(&all));
}

/// Claude Desktop's account differs from Claude Code's, as of the last refresh.
pub fn differs() -> bool {
    DIFFERS.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn availability(id: &str) -> Option<DesktopAvailability> {
    AVAILABILITY.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(id).copied())
}

/// A Claude Desktop switch is running: only one at a time.
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Holds the switch; dropping it (also while a panic unwinds) frees it for the next one.
pub struct DesktopGuard(());

impl Drop for DesktopGuard {
    fn drop(&mut self) {
        BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Claims the switch; `None` while another one runs.
pub fn try_begin() -> Option<DesktopGuard> {
    use std::sync::atomic::Ordering;
    BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| DesktopGuard(()))
}

pub fn is_busy() -> bool {
    BUSY.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    static BUSY_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn desktop_accounts_decrypts_session_key() {
        let json = format!(
            r#"{{"id":"a","name":"A","chromeProfilePath":"p","plan":"Pro","status":"active","sessionKey":"{}"}}"#,
            claude_dashboard_core::store::encrypt_session_key("sk")
        );
        let a = Account::from_json_object(&json).expect("account json");
        let out = desktop_accounts(&[a]);
        assert_eq!(out[0].id, "a");
        assert_eq!(out[0].session_key.as_deref(), Some("sk"));
    }

    #[test]
    fn differs_rule_needs_both_known_and_unequal() {
        assert!(differs_rule(Some("a"), Some("b")));
        assert!(!differs_rule(Some("a"), Some("a")));
        assert!(!differs_rule(None, Some("b")));
        assert!(!differs_rule(Some("a"), None));
        assert!(!differs_rule(None, None));
    }

    #[test]
    fn make_is_none_when_disabled() {
        assert!(make(false).is_none());
    }

    #[test]
    fn busy_flag_is_exclusive_and_released_by_the_guard() {
        let _l = BUSY_TEST.lock().unwrap_or_else(|e| e.into_inner());
        let g = try_begin().expect("free");
        assert!(is_busy());
        assert!(try_begin().is_none());
        drop(g);
        assert!(!is_busy());
    }
}
