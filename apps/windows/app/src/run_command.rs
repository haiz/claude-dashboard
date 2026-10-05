//! Run Command panel controller (ports RunCommandSheet.swift): debounced
//! classify, run hidden or in a terminal, cancel. Store and process work runs
//! on worker threads; the UI is only touched via `invoke_from_event_loop`.
//! The session key is never involved.

use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::Duration;

use claude_dashboard_core::command_classifier::CommandKind;
use claude_dashboard_core::command_log::CommandTrigger;
use claude_dashboard_core::run_commands::{self, RunCommand};
use claude_dashboard_core::settings;
use slint::{ComponentHandle, Model, SharedString};

use crate::runner::CancelToken;
use crate::{commands, log_view, shell, AppWindow};

const DEBOUNCE: Duration = Duration::from_millis(350);
const KEEP_LINES: usize = 2;

#[derive(Default)]
struct State {
    account_id: String,
    user_touched: bool,
    generation: u64,
    cancel: Option<CancelToken>,
    run_seq: u64,
    /// Id of the run the panel is showing; None after cancel or completion.
    current_run: Option<u64>,
    debounce: slint::Timer,
}

/// Does a run's output/completion still belong to the panel?
fn is_current(current: Option<u64>, run: u64) -> bool {
    current == Some(run)
}

/// May a classify result be applied to the toggle?
fn classify_applies(gen: u64, cur_gen: u64, user_touched: bool, text: &str, cur_text: &str) -> bool {
    gen == cur_gen && !user_touched && text == cur_text
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Split a chunk on `\n`/`\r`, dropping empty lines.
fn split_lines(chunk: &str) -> Vec<String> {
    chunk
        .split(['\n', '\r'])
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Append `new` to `current`, keeping only the last `KEEP_LINES`.
fn tail_lines(mut current: Vec<String>, new: Vec<String>) -> Vec<String> {
    current.extend(new);
    let skip = current.len().saturating_sub(KEEP_LINES);
    current.split_off(skip)
}

fn set_lines(app: &AppWindow, lines: Vec<String>) {
    let items: Vec<SharedString> = lines.into_iter().map(Into::into).collect();
    app.set_run_lines(slint::ModelRc::new(slint::VecModel::from(items)));
}

fn close(app: &AppWindow) {
    app.set_run_open(false);
    STATE.with(|s| s.borrow().debounce.stop());
}

/// Debounce, then classify off the UI thread and apply if still current.
fn schedule_classify(weak: slint::Weak<AppWindow>) {
    STATE.with(|s| {
        s.borrow().debounce.start(slint::TimerMode::SingleShot, DEBOUNCE, move || {
            let Some(app) = weak.upgrade() else { return };
            let text = app.get_run_command_text().to_string();
            let gen = STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.generation += 1;
                s.generation
            });
            if text.trim().is_empty() {
                return;
            }
            let weak = weak.clone();
            std::thread::spawn(move || {
                let shell = shell::detect(settings::load().shell.as_deref());
                let kind = commands::classify(&text, shell.as_ref());
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(app) = weak.upgrade() else { return };
                    let cur_text = app.get_run_command_text();
                    let ok = STATE.with(|s| {
                        let s = s.borrow();
                        classify_applies(gen, s.generation, s.user_touched, &text, &cur_text)
                    });
                    if ok {
                        app.set_run_in_terminal(kind == CommandKind::Interactive);
                    }
                });
            });
        });
    });
}

fn account_name(app: &AppWindow, id: &str) -> String {
    app.get_account_rows()
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.name.to_string())
        .unwrap_or_default()
}

fn open(weak: slint::Weak<AppWindow>, id: String) {
    let Some(app) = weak.upgrade() else { return };
    app.set_run_account_name(account_name(&app, &id).into());
    STATE.with(|s| s.borrow_mut().account_id = id.clone());
    std::thread::spawn(move || {
        let saved = run_commands::get(&id).unwrap_or_default();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(app) = weak.upgrade() else { return };
            STATE.with(|s| s.borrow_mut().user_touched = false);
            let has_text = !saved.command.is_empty();
            app.set_run_command_text(saved.command.into());
            app.set_run_in_terminal(saved.open_in_terminal);
            set_lines(&app, Vec::new());
            // A cancelled run's worker may still be alive; its state is not ours.
            if STATE.with(|s| s.borrow().current_run.is_none()) {
                app.set_run_running(false);
            }
            app.set_run_open(true);
            if has_text {
                schedule_classify(weak.clone());
            }
        });
    });
}

fn start(weak: slint::Weak<AppWindow>, nudge: Sender<()>) {
    let Some(app) = weak.upgrade() else { return };
    let command = app.get_run_command_text().to_string();
    if command.trim().is_empty() || app.get_run_running() {
        return;
    }
    let open_in_terminal = app.get_run_in_terminal();
    let id = STATE.with(|s| s.borrow().account_id.clone());

    if open_in_terminal {
        std::thread::spawn(move || {
            save(&id, &command, true);
            let shell = shell::detect(settings::load().shell.as_deref());
            commands::launch_in_terminal(&command, Some(&id), CommandTrigger::Manual, shell.as_ref());
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = weak.upgrade() {
                    close(&app);
                }
                let _ = nudge.send(());
                log_view::reload(&weak);
            });
        });
        return;
    }

    app.set_run_running(true);
    set_lines(&app, Vec::new());
    let token = CancelToken::new();
    let run = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.run_seq += 1;
        s.cancel = Some(token.clone());
        s.current_run = Some(s.run_seq);
        s.run_seq
    });
    std::thread::spawn(move || {
        save(&id, &command, false);
        let shell = shell::detect(settings::load().shell.as_deref());
        let sink = Mutex::new(weak.clone());
        let on_output = move |chunk: &str| {
            let new = split_lines(chunk);
            if new.is_empty() {
                return;
            }
            let w = sink.lock().map(|g| g.clone()).ok();
            let Some(w) = w else { return };
            let _ = slint::invoke_from_event_loop(move || {
                if !STATE.with(|s| is_current(s.borrow().current_run, run)) {
                    return;
                }
                if let Some(app) = w.upgrade() {
                    let cur: Vec<String> =
                        app.get_run_lines().iter().map(|s| s.to_string()).collect();
                    set_lines(&app, tail_lines(cur, new));
                }
            });
        };
        commands::execute(&command, Some(&id), CommandTrigger::Manual, shell.as_ref(), &token, &on_output);
        let _ = slint::invoke_from_event_loop(move || {
            // A cancelled (superseded) run already reset the UI; touch it only if current.
            if STATE.with(|s| is_current(s.borrow().current_run, run)) {
                if let Some(app) = weak.upgrade() {
                    app.set_run_running(false);
                    close(&app);
                }
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.current_run = None;
                    s.cancel = None;
                });
            }
            let _ = nudge.send(());
            log_view::reload(&weak);
        });
    });
}

fn save(id: &str, command: &str, open_in_terminal: bool) {
    let rc = RunCommand { command: command.to_string(), open_in_terminal };
    if let Err(e) = run_commands::set(id, &rc) {
        eprintln!("save run command failed: {e}");
    }
}

pub fn install(app: &AppWindow, nudge: Sender<()>) {
    let w = app.as_weak();
    app.on_open_run_command(move |id| open(w.clone(), id.to_string()));

    let w = app.as_weak();
    app.on_run_command_edited(move |_| {
        STATE.with(|s| s.borrow_mut().user_touched = false);
        schedule_classify(w.clone());
    });

    let w = app.as_weak();
    app.on_run_terminal_toggled(move |on| {
        STATE.with(|s| s.borrow_mut().user_touched = true);
        if let Some(app) = w.upgrade() {
            app.set_run_in_terminal(on);
        }
    });

    let w = app.as_weak();
    app.on_run_command_start(move || start(w.clone(), nudge.clone()));

    let w = app.as_weak();
    app.on_run_command_cancel(move || {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(t) = s.cancel.take() {
                t.cancel();
            }
            s.current_run = None;
        });
        if let Some(app) = w.upgrade() {
            app.set_run_running(false);
            close(&app);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_split_on_newlines_and_carriage_returns() {
        assert_eq!(split_lines("a\r\nb\n\nc\r"), vec!["a", "b", "c"]);
        assert!(split_lines("\r\n").is_empty());
    }

    #[test]
    fn run_currency() {
        assert!(is_current(Some(3), 3));
        assert!(!is_current(Some(4), 3));
        assert!(!is_current(None, 3));
    }

    #[test]
    fn classify_apply_rules() {
        assert!(classify_applies(2, 2, false, "ls", "ls"));
        assert!(!classify_applies(1, 2, false, "ls", "ls"));
        assert!(!classify_applies(2, 2, true, "ls", "ls"));
        assert!(!classify_applies(2, 2, false, "ls", "ls -l"));
    }

    #[test]
    fn only_the_last_two_lines_are_kept() {
        let v = tail_lines(vec!["1".into()], vec!["2".into(), "3".into()]);
        assert_eq!(v, vec!["2", "3"]);
        assert_eq!(tail_lines(vec![], vec!["x".into()]), vec!["x"]);
    }
}
