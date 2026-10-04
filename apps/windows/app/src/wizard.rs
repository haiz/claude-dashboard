//! Add Account wizard wiring. All blocking work (scan, add, store polling)
//! runs on worker threads; results return via `invoke_from_event_loop`.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use claude_dashboard_core::scan::{scan_windows_profiles, ProfileScanStatus, ScannedSession};
use claude_dashboard_core::store;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::setup::{self, AddOutcome};
use crate::{ScanItem, SetupWizardWindow};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const WAITING: &str = "Waiting for a key\u{2026}";

/// Human text for one add outcome (never contains the key).
pub fn outcome_text(o: &AddOutcome) -> String {
    match o {
        AddOutcome::Added(n) => format!("Added {n}"),
        AddOutcome::Updated(n) => format!("Updated {n}"),
        AddOutcome::Rejected(r) => format!("Not accepted: {r}"),
        AddOutcome::NoChatOrg => {
            "This account has no chat-capable organization, so it was not added.".to_string()
        }
        AddOutcome::StoreError(e) => format!("Could not save the account: {e}"),
    }
}

fn sync_count(w: &SetupWizardWindow) {
    let m = w.get_scan_items();
    let n = (0..m.row_count())
        .filter_map(|i| m.row_data(i))
        .filter(|it| it.idx >= 0 && it.checked)
        .count();
    w.set_selected_count(n as i32);
}

fn is_success(o: &AddOutcome) -> bool {
    matches!(o, AddOutcome::Added(_) | AddOutcome::Updated(_))
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
pub fn install(dark: bool, nudge: Sender<()>) -> Result<Rc<dyn Fn()>, slint::PlatformError> {
    let win = SetupWizardWindow::new()?;
    win.global::<crate::Theme>().set_dark(dark);

    let timer = Rc::new(slint::Timer::default());
    // Account ids present when the wizard opened / last reported; shared with workers.
    let baseline: Arc<Mutex<Option<HashSet<String>>>> = Arc::new(Mutex::new(None));
    let polling = Arc::new(AtomicBool::new(false));
    // Sessions behind the scan rows (ScanItem.idx indexes this).
    let found: Arc<Mutex<Vec<ScannedSession>>> = Arc::new(Mutex::new(Vec::new()));

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
                        let _ = nudge.send(());
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

    // Stop the poll when the tab changes away (restart when it returns).
    {
        let (timer, start_poll) = (timer.clone(), start_poll.clone());
        win.on_tab_changed(move |t| {
            if t == 0 {
                start_poll();
            } else {
                timer.stop();
            }
        });
    }

    // ---- scan tab ----
    {
        let (weak, found) = (win.as_weak(), found.clone());
        win.on_scan(move || {
            let Some(w) = weak.upgrade() else { return };
            w.set_scanning(true);
            w.set_scan_message(SharedString::from(""));
            w.set_scan_items(ModelRc::new(VecModel::<ScanItem>::default()));
            let (weak, found) = (weak.clone(), found.clone());
            std::thread::spawn(move || {
                let results = scan_windows_profiles();
                let mut sessions = Vec::new();
                let mut rows: Vec<(i32, String, String)> = Vec::new();
                let mut app_bound = 0usize;
                for r in results {
                    match r {
                        ProfileScanStatus::Found(s) => {
                            let label = match &s.google_email {
                                Some(e) if !e.is_empty() => format!("{} ({e})", s.display_name),
                                _ => s.display_name.clone(),
                            };
                            let detail = format!("{:?} - {}", s.browser, s.profile_dir);
                            rows.push((sessions.len() as i32, label, detail));
                            sessions.push(s);
                        }
                        ProfileScanStatus::AppBound => {
                            app_bound += 1;
                            rows.push((
                                -1,
                                "App-bound browser profile".to_string(),
                                "Protected cookies - use the browser extension instead".to_string(),
                            ));
                        }
                        ProfileScanStatus::NoSession => {}
                    }
                }
                let n_found = sessions.len();
                if let Ok(mut f) = found.lock() {
                    *f = sessions;
                }
                let message = if rows.is_empty() {
                    "No Claude sessions found. Sign in to claude.ai in your browser, or use the extension.".to_string()
                } else if n_found == 0 && app_bound > 0 {
                    "Your browser protects its cookies; use the browser extension tab.".to_string()
                } else {
                    String::new()
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = weak.upgrade() else { return };
                    let items: Vec<ScanItem> = rows
                        .into_iter()
                        .map(|(idx, label, detail)| ScanItem {
                            idx,
                            label: label.into(),
                            detail: detail.into(),
                            checked: idx >= 0,
                        })
                        .collect();
                    w.set_scan_items(ModelRc::new(VecModel::from(items)));
                    sync_count(&w);
                    w.set_scan_message(SharedString::from(message));
                    w.set_scanning(false);
                });
            });
        });
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
            let model = w.get_scan_items();
            let selected: Vec<ScannedSession> = {
                let sessions = found.lock().map(|f| f.clone()).unwrap_or_default();
                (0..model.row_count())
                    .filter_map(|i| model.row_data(i))
                    .filter(|it| it.idx >= 0 && it.checked)
                    .filter_map(|it| sessions.get(it.idx as usize).cloned())
                    .collect()
            };
            if selected.is_empty() {
                return;
            }
            w.set_adding(true);
            let (weak, nudge) = (weak.clone(), nudge.clone());
            std::thread::spawn(move || {
                let outcomes = setup::add_scanned(&selected);
                if outcomes.iter().any(is_success) {
                    let _ = nudge.send(());
                }
                let text = outcomes
                    .iter()
                    .zip(&selected)
                    .map(|(o, s)| format!("{}: {}", s.display_name, outcome_text(o)))
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_scan_message(SharedString::from(text));
                        w.set_adding(false);
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
            w.set_paste_result(SharedString::from(""));
            let key = key.trim().to_string();
            let (weak, nudge) = (weak.clone(), nudge.clone());
            std::thread::spawn(move || {
                let outcome = setup::add_from_session_key(&key);
                let ok = is_success(&outcome);
                if ok {
                    let _ = nudge.send(());
                }
                let text = outcome_text(&outcome);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_paste_ok(ok);
                        w.set_paste_result(SharedString::from(text));
                        w.set_pasting(false);
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
        win.set_paste_result(SharedString::from(""));
        win.set_scan_message(SharedString::from(""));
        win.set_scan_items(ModelRc::new(VecModel::<ScanItem>::default()));
        win.set_selected_count(0);
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
