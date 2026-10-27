//! The Claude Desktop step of the Switch flow (macOS `switchDesktop`, `confirmDesktopClose`,
//! `runDesktopSwitch`): the account page's button, the close prompt's confirm, and the
//! worker that quits and switches Desktop. Texts and rules live in `cd_flow`.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use claude_dashboard_core::cc_switch::model::VaultEntry;
use claude_dashboard_core::cd_switch::switcher::DesktopSwitchError;
use claude_dashboard_core::model::Account;

use crate::{cc_flow, cc_login, cd_flow, cd_state};

/// Claude Desktop runs now; false without a switcher (dev runs, not installed).
pub fn desktop_running() -> bool {
    cd_state::switcher().is_some_and(|s| s.is_running())
}

/// The "Switch Claude Desktop with Claude Code" setting, only when there is a switcher.
pub fn enabled() -> bool {
    cd_state::switcher().is_some() && claude_dashboard_core::settings::load().switch_desktop_enabled
}

/// UI thread: the account page's "Switch Desktop to this account". Quits nothing by itself:
/// a running Desktop asks first, a closed one is switched straight away.
pub fn request(id: String) {
    let Some(s) = cd_state::switcher() else { return };
    if cd_state::is_busy() {
        return;
    }
    if s.is_running() {
        crate::cd_ask_close(id);
    } else {
        start(id, false);
    }
}

/// UI thread: runs the Desktop step on a worker (`quit_first` once the user agreed to close
/// it), then shows its message, or nothing when the user cancelled the browser wait.
pub fn start(id: String, quit_first: bool) {
    let Some(guard) = cd_state::try_begin() else { return };
    set_switching(true);
    let cancel = Arc::new(AtomicBool::new(false));
    crate::cc_set_cancel(Some(cancel.clone()));
    std::thread::spawn(move || {
        let message = run(&id, quit_first, &cancel);
        drop(guard);
        let _ = slint::invoke_from_event_loop(move || {
            set_switching(false);
            crate::cc_set_cancel(None);
            match message {
                Some(m) => crate::cc_prompt(2, cc_flow::MESSAGE_TITLE, &m),
                None => crate::cc_prompt(0, "", ""),
            }
        });
    });
}

/// Worker, right after a Claude Code switch with Desktop closed and the setting on: the
/// Desktop message to add under the Claude Code one. `None` when another Desktop switch runs
/// or the user cancelled the browser wait.
pub fn run_now(id: &str, cancel: &AtomicBool) -> Option<String> {
    let guard = cd_state::try_begin()?;
    let _ = slint::invoke_from_event_loop(|| set_switching(true));
    let message = run(id, false, cancel);
    drop(guard);
    let _ = slint::invoke_from_event_loop(|| set_switching(false));
    message
}

fn set_switching(on: bool) {
    crate::cc_ui(|a| a.set_desktop_switching(on), |p| p.set_desktop_switching(on));
}

/// Worker: quits Desktop first when `quit_first` (consent already given), then switches it,
/// logging the attempt; then recomputes whether Desktop differs from Claude Code.
fn run(id: &str, quit_first: bool, cancel: &AtomicBool) -> Option<String> {
    let accounts = claude_dashboard_core::store::load_accounts().unwrap_or_default();
    let account = accounts.iter().find(|a| a.id == id)?;
    let message = attempt(account, &accounts, quit_first, cancel);
    refresh_differs(&accounts);
    message
}

fn attempt(account: &Account, accounts: &[Account], quit_first: bool, cancel: &AtomicBool) -> Option<String> {
    let switcher = cd_state::switcher()?;
    let name = account.email.clone().unwrap_or_else(|| account.name.clone());
    // Blocks up to ~6 s: always on a worker.
    if quit_first && switcher.is_running() && !switcher.quit() {
        return Some(cd_flow::did_not_close_message(&name));
    }
    let all = cd_state::desktop_accounts(accounts);
    let target = all.iter().find(|a| a.id == account.id)?;
    // Without a browser profile no capture is possible: core then reports NotCaptured /
    // LoginExpired, worded as "Re-sync first".
    let sources = claude_dashboard_core::extension_sources::load(&claude_dashboard_core::extension_sources::sources_path())
        .unwrap_or_default();
    let input = cc_flow::provision_input(account, &sources);
    let cancelled = Cell::new(false);
    let capture = input.as_ref().map(|input| {
        let (name, cancelled) = (&name, &cancelled);
        move || -> Result<VaultEntry, DesktopSwitchError> {
            let on_awaiting = || {
                let _ = slint::invoke_from_event_loop(|| crate::cc_prompt(3, cc_flow::WAITING_TITLE, cc_flow::WAITING_TEXT));
            };
            match cc_login::provision(input, cancel, on_awaiting, &cc_login::RealDeps, cc_login::BROWSER_TIMEOUT) {
                Ok(entry) if !cancel.load(Ordering::SeqCst) => Ok(entry),
                Ok(_) => {
                    cancelled.set(true);
                    Err(DesktopSwitchError::Provision(String::new()))
                }
                Err(e) => Err(DesktopSwitchError::Provision(e.message(name).unwrap_or_else(|| {
                    cancelled.set(true);
                    String::new()
                }))),
            }
        }
    });
    let provision = capture.as_ref().map(|f| f as &dyn Fn() -> Result<VaultEntry, DesktopSwitchError>);
    let result = switcher.switch_to(target, &all, provision);
    // A cancelled browser wait shows nothing and is not logged (as for Claude Code).
    if result.is_err() && cancelled.get() {
        return None;
    }
    if let Ok(log) = claude_dashboard_core::auto_switch_log::AutoSwitchLogStore::open() {
        log.insert(&cd_flow::log_entry(&name, result.as_ref().err(), crate::unix_now()));
    }
    Some(match result {
        Ok(()) => cd_flow::success_message(&name),
        Err(e) => cd_flow::error_message(&e, &name, input.is_some()),
    })
}

/// Worker: recomputes whether Desktop differs from Claude Code's active account and shows it.
fn refresh_differs(accounts: &[Account]) {
    let email = claude_dashboard_core::claude_code::active_email();
    let cc_uuid = email
        .and_then(|e| accounts.iter().find(|a| a.email.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(&e))))
        .and_then(|a| a.account_uuid.clone());
    cd_state::refresh(accounts, cc_uuid.as_deref());
    push_differs();
}

/// Shows `cd_state::differs()` in both windows; callable from any thread.
pub fn push_differs() {
    let on = cd_state::differs();
    let _ = slint::invoke_from_event_loop(move || {
        crate::cc_ui(|a| a.set_desktop_differs(on), |p| p.set_desktop_differs(on));
    });
}
