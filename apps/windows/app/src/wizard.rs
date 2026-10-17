//! Add Account wizard wiring. All blocking work (scan, add, store polling)
//! runs on worker threads; results return via `invoke_from_event_loop`.
//!
//! The Scan and Paste tabs follow macOS `SetupView` / `PasteKeyView`: the scan
//! reads one browser, picked once and remembered in `preferredScanBrowser`, and
//! a successful add closes the window.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use claude_dashboard_core::model::Browser;
use claude_dashboard_core::plan::plan_wire_value;
use claude_dashboard_core::scan::{installed_windows_browsers, scan_windows_browser};
use claude_dashboard_core::{settings, store};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::setup::{self, DetectedAccount, RealScanEnv};
use crate::{ScanItem, SetupWizardWindow};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const WAITING: &str = "Waiting for a key\u{2026}";

/// The `preferredScanBrowser` value of a browser (its stored raw value).
fn browser_key(b: &Browser) -> &'static str {
    match b {
        Browser::Chrome => "chrome",
        Browser::ChromeDev => "chromeDev",
        Browser::Arc => "arc",
        Browser::Brave => "brave",
        Browser::Edge => "edge",
    }
}

/// Which browser the Scan tab starts on, and whether it must ask first
/// (macOS `SetupView.onAppear`): a saved choice that is still installed, else
/// the only browser, else ask when there are two or more.
fn initial_browser(installed: &[Browser], saved: Option<&str>) -> (usize, bool) {
    if let Some(i) = saved.and_then(|k| installed.iter().position(|b| browser_key(b) == k)) {
        return (i, false);
    }
    (0, installed.len() >= 2)
}

fn remember_browser(b: &Browser) {
    let key = browser_key(b).to_string();
    let _ = settings::update(|s| s.preferred_scan_browser = Some(key));
}

fn scan_item(idx: usize, d: &DetectedAccount) -> ScanItem {
    ScanItem {
        idx: idx as i32,
        label: d.name.clone().into(),
        detail: d.detail().into(),
        plan: d.plan.as_ref().map(plan_wire_value).unwrap_or_default().into(),
        checked: true,
    }
}

fn sync_count(w: &SetupWizardWindow) {
    let m = w.get_scan_items();
    let n = (0..m.row_count())
        .filter_map(|i| m.row_data(i))
        .filter(|it| it.checked)
        .count();
    w.set_selected_count(n as i32);
}

/// (id, display name) of every stored account; empty on a read error.
fn account_ids() -> Vec<(String, String)> {
    store::load_accounts()
        .map(|v| {
            v.into_iter()
                .map(|a| {
                    let name = a.email.clone().filter(|e| !e.is_empty()).unwrap_or(a.name);
                    (a.id, name)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Builds the (hidden) wizard window and returns the function that opens it.
pub fn install(
    dark: bool,
    nudge: Sender<crate::refresh::Nudge>,
) -> Result<Rc<dyn Fn()>, slint::PlatformError> {
    let win = SetupWizardWindow::new()?;
    win.global::<crate::Theme>().set_dark(dark);

    let timer = Rc::new(slint::Timer::default());
    // Account ids present when the wizard opened / last reported; shared with workers.
    let baseline: Arc<Mutex<Option<HashSet<String>>>> = Arc::new(Mutex::new(None));
    let polling = Arc::new(AtomicBool::new(false));
    // Accounts behind the scan rows (ScanItem.idx indexes this).
    let found: Arc<Mutex<Vec<DetectedAccount>>> = Arc::new(Mutex::new(Vec::new()));
    // Installed browsers, in picker order; read when the wizard opens.
    let installed: Rc<RefCell<Vec<Browser>>> = Rc::new(RefCell::new(Vec::new()));
    // Bumped by every scan (and on open), so a superseded scan's result is dropped
    // (macOS cancels the previous `scanTask`).
    let generation = Arc::new(AtomicU64::new(0));

    // ---- extension tab poll ----
    let start_poll: Rc<dyn Fn()> = {
        let (timer, baseline, polling) = (timer.clone(), baseline.clone(), polling.clone());
        let (weak, nudge) = (win.as_weak(), nudge.clone());
        Rc::new(move || {
            let (baseline, polling) = (baseline.clone(), polling.clone());
            let (weak, nudge) = (weak.clone(), nudge.clone());
            timer.start(slint::TimerMode::Repeated, POLL_INTERVAL, move || {
                let Some(w) = weak.upgrade() else { return };
                if w.get_tab() != 0 || polling.swap(true, Ordering::SeqCst) {
                    return;
                }
                let (baseline, polling) = (baseline.clone(), polling.clone());
                let (weak, nudge) = (weak.clone(), nudge.clone());
                std::thread::spawn(move || {
                    let current = account_ids();
                    let mut new_name = None;
                    if let Ok(mut b) = baseline.lock() {
                        if let Some(known) = b.as_mut() {
                            for (id, name) in &current {
                                if known.insert(id.clone()) {
                                    new_name = Some(name.clone());
                                }
                            }
                        }
                    }
                    polling.store(false, Ordering::SeqCst);
                    if let Some(name) = new_name {
                        let _ = nudge.send(crate::refresh::Nudge::All);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak.upgrade() {
                                w.set_ext_status(SharedString::from(format!("Added {name}")));
                                w.set_ext_done(true);
                            }
                        });
                    }
                });
            });
        })
    };

    // ---- scan tab ----
    // Scans the picked browser: read its profiles, check each key with claude.ai,
    // offer the accounts not stored yet and explain the rest.
    let start_scan: Rc<dyn Fn()> = {
        let (weak, found, installed, generation) =
            (win.as_weak(), found.clone(), installed.clone(), generation.clone());
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            let gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
            w.set_scan_items(ModelRc::new(VecModel::<ScanItem>::default()));
            w.set_selected_count(0);
            let browser = installed.borrow().get(w.get_browser_index().max(0) as usize).cloned();
            let Some(browser) = browser else {
                w.set_scan_message(SharedString::from(setup::NO_BROWSER_MESSAGE));
                w.set_scanning(false);
                return;
            };
            w.set_scan_message(SharedString::from(""));
            w.set_scanning(true);
            let (weak, found, generation) = (weak.clone(), found.clone(), generation.clone());
            std::thread::spawn(move || {
                let profiles = scan_windows_browser(browser.clone());
                let stored = setup::stored_identities();
                let (offered, message) =
                    match setup::detect_accounts(profiles, &stored, &RealScanEnv) {
                        None => (Vec::new(), setup::no_sessions_message(&browser)),
                        Some((offered, skipped)) => {
                            let m = setup::scan_summary(offered.len(), &skipped).unwrap_or_default();
                            (offered, m)
                        }
                    };
                if generation.load(Ordering::SeqCst) != gen {
                    return;
                }
                let items: Vec<ScanItem> =
                    offered.iter().enumerate().map(|(i, d)| scan_item(i, d)).collect();
                if let Ok(mut f) = found.lock() {
                    *f = offered;
                }
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = weak.upgrade() else { return };
                    if generation.load(Ordering::SeqCst) != gen {
                        return;
                    }
                    w.set_scan_items(ModelRc::new(VecModel::from(items)));
                    sync_count(&w);
                    w.set_scan_message(SharedString::from(message));
                    w.set_scanning(false);
                });
            });
        })
    };

    // Entering the Scan tab scans once per opening, unless a browser must be picked
    // first; leaving the Extension tab stops its poll (restart when it returns).
    {
        let (timer, start_poll, start_scan) = (timer.clone(), start_poll.clone(), start_scan.clone());
        let weak = win.as_weak();
        win.on_tab_changed(move |t| {
            if t == 0 {
                start_poll();
            } else {
                timer.stop();
            }
            let Some(w) = weak.upgrade() else { return };
            let fresh = !w.get_scanning()
                && w.get_scan_items().row_count() == 0
                && w.get_scan_message().is_empty();
            if t == 1 && !w.get_choosing_browser() && fresh {
                start_scan();
            }
        });
    }
    {
        let (weak, installed, start_scan) = (win.as_weak(), installed.clone(), start_scan.clone());
        win.on_choose_browser(move |i| {
            let Some(w) = weak.upgrade() else { return };
            if let Some(b) = installed.borrow().get(i.max(0) as usize) {
                remember_browser(b);
            }
            w.set_browser_index(i);
            w.set_choosing_browser(false);
            start_scan();
        });
    }
    {
        let (installed, start_scan) = (installed.clone(), start_scan.clone());
        win.on_browser_changed(move |i| {
            if let Some(b) = installed.borrow().get(i.max(0) as usize) {
                remember_browser(b);
            }
            start_scan();
        });
    }
    {
        let start_scan = start_scan.clone();
        win.on_scan(move || start_scan());
    }
    {
        let weak = win.as_weak();
        win.on_toggle(move |i| {
            let Some(w) = weak.upgrade() else { return };
            let model = w.get_scan_items();
            if let Some(vm) = model.as_any().downcast_ref::<VecModel<ScanItem>>() {
                if let Some(mut row) = vm.row_data(i as usize) {
                    row.checked = !row.checked;
                    vm.set_row_data(i as usize, row);
                }
            }
            sync_count(&w);
        });
    }
    {
        let (weak, found, nudge) = (win.as_weak(), found.clone(), nudge.clone());
        win.on_add_selected(move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_adding() {
                return;
            }
            let model = w.get_scan_items();
            let selected: Vec<DetectedAccount> = {
                let offered = found.lock().map(|f| f.clone()).unwrap_or_default();
                (0..model.row_count())
                    .filter_map(|i| model.row_data(i))
                    .filter(|it| it.checked)
                    .filter_map(|it| offered.get(it.idx as usize).cloned())
                    .collect()
            };
            if selected.is_empty() {
                return;
            }
            w.set_adding(true);
            w.set_scan_message(SharedString::from(""));
            let (weak, nudge) = (weak.clone(), nudge.clone());
            std::thread::spawn(move || {
                let result = setup::add_detected(&selected);
                if result.is_ok() {
                    let _ = nudge.send(crate::refresh::Nudge::All);
                }
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = weak.upgrade() else { return };
                    w.set_adding(false);
                    match result {
                        // macOS adds, refreshes and closes the sheet.
                        Ok(()) => w.invoke_done(),
                        Err(e) => w.set_scan_message(SharedString::from(e)),
                    }
                });
            });
        });
    }

    // ---- paste tab ----
    {
        let (weak, nudge) = (win.as_weak(), nudge.clone());
        win.on_paste_add(move |key| {
            let Some(w) = weak.upgrade() else { return };
            if w.get_pasting() {
                return;
            }
            w.set_pasting(true);
            w.set_paste_hold(false);
            w.set_paste_result(SharedString::from(""));
            let key = key.to_string();
            let (weak, nudge) = (weak.clone(), nudge.clone());
            std::thread::spawn(move || {
                let outcome = setup::add_from_session_key(&key);
                if !outcome.is_failure() {
                    let _ = nudge.send(crate::refresh::Nudge::All);
                }
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = weak.upgrade() else { return };
                    w.set_pasting(false);
                    w.set_paste_ok(!outcome.is_failure());
                    w.set_paste_result(SharedString::from(outcome.message()));
                    if outcome.is_failure() {
                        return;
                    }
                    // macOS PasteKeyView: clear the key; close unless a warning must be read.
                    w.set_paste_key(SharedString::from(""));
                    if outcome.holds_open() {
                        w.set_paste_hold(true);
                    } else {
                        w.invoke_done();
                    }
                });
            });
        });
    }

    // ---- close: always stop the poll timer ----
    let close: Rc<dyn Fn()> = {
        let (weak, timer) = (win.as_weak(), timer.clone());
        Rc::new(move || {
            timer.stop();
            if let Some(w) = weak.upgrade() {
                let _ = w.hide();
            }
        })
    };
    {
        let close = close.clone();
        win.on_done(move || close());
    }
    {
        let close = close.clone();
        win.window().on_close_requested(move || {
            close();
            slint::CloseRequestResponse::HideWindow
        });
    }

    // ---- opener ----
    let win = Rc::new(win);
    Ok(Rc::new(move || {
        win.set_tab(0);
        win.set_ext_status(SharedString::from(WAITING));
        win.set_ext_done(false);
        win.set_paste_key(SharedString::from(""));
        win.set_paste_result(SharedString::from(""));
        win.set_paste_ok(false);
        win.set_paste_hold(false);
        // Drop any scan still running from a previous opening.
        generation.fetch_add(1, Ordering::SeqCst);
        win.set_scanning(false);
        win.set_adding(false);
        win.set_scan_message(SharedString::from(""));
        win.set_scan_items(ModelRc::new(VecModel::<ScanItem>::default()));
        win.set_selected_count(0);
        let browsers = installed_windows_browsers();
        let (index, choosing) =
            initial_browser(&browsers, settings::load().preferred_scan_browser.as_deref());
        let names: Vec<SharedString> =
            browsers.iter().map(|b| SharedString::from(b.display_name())).collect();
        win.set_browsers(ModelRc::new(VecModel::from(names)));
        win.set_browser_index(index as i32);
        win.set_choosing_browser(choosing);
        *installed.borrow_mut() = browsers;
        if win.show().is_err() {
            return;
        }
        // Baseline snapshot off-thread, then poll for new accounts.
        let baseline = baseline.clone();
        if let Ok(mut b) = baseline.lock() {
            *b = None;
        }
        std::thread::spawn(move || {
            let ids: HashSet<String> = account_ids().into_iter().map(|(id, _)| id).collect();
            if let Ok(mut b) = baseline.lock() {
                *b = Some(ids);
            }
        });
        start_poll();
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_installed_browser_is_used_without_asking() {
        let installed = [Browser::Chrome, Browser::Edge, Browser::Brave];
        assert_eq!(initial_browser(&installed, Some("edge")), (1, false));
    }

    #[test]
    fn two_browsers_and_no_usable_choice_ask_first() {
        let installed = [Browser::Chrome, Browser::Edge];
        assert_eq!(initial_browser(&installed, None), (0, true));
        // Saved but since uninstalled: ask again, as macOS does.
        assert_eq!(initial_browser(&installed, Some("brave")), (0, true));
    }

    #[test]
    fn one_or_no_browser_never_asks() {
        assert_eq!(initial_browser(&[Browser::Brave], None), (0, false));
        assert_eq!(initial_browser(&[], None), (0, false));
    }

    #[test]
    fn browser_keys_are_the_stored_raw_values() {
        assert_eq!(browser_key(&Browser::ChromeDev), "chromeDev");
        assert_eq!(browser_key(&Browser::Edge), "edge");
    }
}
