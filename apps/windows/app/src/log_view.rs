//! Command Log pane glue: pure mapping from log rows to UI rows, plus the
//! load/clear plumbing (off the UI thread).

use std::collections::HashMap;

use claude_dashboard_core::command_log::{
    CommandLogEntry, CommandLogStore, CommandStatus, CommandTrigger,
};

use slint::ComponentHandle;

use crate::{AppWindow, UiLogEntry};

/// Civil date from days since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `"2026-10-05 10:42:03"` for a Unix time already shifted to local.
pub fn format_log_time(local_unix: i64) -> String {
    let days = local_unix.div_euclid(86_400);
    let secs = local_unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

pub fn duration_text(e: &CommandLogEntry) -> String {
    match e.finished_unix {
        Some(f) => format!("{:.1}s", (f - e.started_unix).max(0) as f64),
        None => "—".to_string(),
    }
}

pub fn status_text(e: &CommandLogEntry) -> String {
    match e.status {
        CommandStatus::Exited => match e.exit_code {
            Some(c) => format!("exit {c}"),
            None => "—".to_string(),
        },
        s => s.label().to_string(),
    }
}

pub fn status_rgb(e: &CommandLogEntry) -> (u8, u8, u8) {
    match e.status {
        CommandStatus::Exited => match e.exit_code {
            Some(0) => (48, 209, 88),
            Some(_) => (255, 69, 58),
            None => (142, 142, 147),
        },
        CommandStatus::Cancelled => (142, 142, 147),
        CommandStatus::TimedOut => (255, 159, 10),
        CommandStatus::LaunchedInTerminal => (10, 132, 255),
        CommandStatus::LaunchFailed => (255, 69, 58),
    }
}

pub fn trigger_rgb(t: CommandTrigger) -> (u8, u8, u8) {
    match t {
        CommandTrigger::Manual => (10, 132, 255),
        CommandTrigger::AutoReset => (191, 90, 242),
        CommandTrigger::AutoEmpty => (255, 159, 10),
    }
}

pub fn account_label(id: Option<&str>, names: &HashMap<String, String>) -> String {
    match id {
        None => "—".to_string(),
        Some(i) => names
            .get(i)
            .cloned()
            .unwrap_or_else(|| "Deleted account".to_string()),
    }
}

fn color((r, g, b): (u8, u8, u8)) -> slint::Color {
    slint::Color::from_rgb_u8(r, g, b)
}

pub fn to_ui_entry(
    e: &CommandLogEntry,
    names: &HashMap<String, String>,
    offset_s: f64,
) -> UiLogEntry {
    let output = e.output.clone().unwrap_or_default();
    let output = if output.is_empty() {
        "No output captured".to_string()
    } else {
        output
    };
    UiLogEntry {
        id: e.id as i32,
        time: format_log_time(e.started_unix + offset_s as i64).into(),
        trigger: e.trigger.label().into(),
        trigger_color: color(trigger_rgb(e.trigger)),
        account: account_label(e.account_id.as_deref(), names).into(),
        command: e.command.clone().into(),
        status: status_text(e).into(),
        status_color: color(status_rgb(e)),
        duration: duration_text(e).into(),
        output: output.into(),
    }
}

pub fn reload(weak: &slint::Weak<AppWindow>) {
    let weak = weak.clone();
    std::thread::spawn(move || {
        let entries = CommandLogStore::open()
            .map(|s| s.recent(500))
            .unwrap_or_default();
        let names: HashMap<String, String> = claude_dashboard_core::store::load_accounts()
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a.name))
            .collect();
        let offset = crate::model::local_offset_s();
        let rows: Vec<_> = entries
            .iter()
            .map(|e| to_ui_entry(e, &names, offset))
            .collect();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(a) = weak.upgrade() {
                a.set_command_log(slint::ModelRc::new(slint::VecModel::from(rows)));
            }
        });
    });
}

pub fn install(app: &AppWindow) {
    let w = app.as_weak();
    app.on_command_log_open(move || reload(&w));
    let w = app.as_weak();
    app.on_command_log_clear(move || {
        let w = w.clone();
        std::thread::spawn(move || {
            if let Ok(s) = CommandLogStore::open() {
                if let Err(e) = s.clear() {
                    eprintln!("clear command log failed: {e}");
                }
            }
            reload(&w);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn e(status: CommandStatus, exit: Option<i32>, finished: Option<i64>) -> CommandLogEntry {
        CommandLogEntry {
            id: 1,
            account_id: Some("a".into()),
            command: "x".into(),
            trigger: CommandTrigger::Manual,
            started_unix: 100,
            finished_unix: finished,
            exit_code: exit,
            status,
            output: None,
        }
    }

    #[test]
    fn time_is_civil_local() {
        assert_eq!(format_log_time(0), "1970-01-01 00:00:00");
        assert_eq!(format_log_time(1_791_196_923), "2026-10-05 10:42:03");
        assert_eq!(format_log_time(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn duration_and_status_text() {
        assert_eq!(duration_text(&e(CommandStatus::Exited, Some(0), Some(102))), "2.0s");
        assert_eq!(duration_text(&e(CommandStatus::Exited, Some(0), None)), "—");
        assert_eq!(
            duration_text(&e(CommandStatus::Exited, Some(0), Some(90))),
            "0.0s",
            "clock skew clamps"
        );
        assert_eq!(status_text(&e(CommandStatus::Exited, Some(3), Some(1))), "exit 3");
        assert_eq!(status_text(&e(CommandStatus::Exited, None, Some(1))), "—");
        assert_eq!(status_text(&e(CommandStatus::TimedOut, None, Some(1))), "Timed out");
    }

    #[test]
    fn status_colors() {
        assert_eq!(status_rgb(&e(CommandStatus::Exited, Some(0), None)), (48, 209, 88));
        assert_eq!(status_rgb(&e(CommandStatus::Exited, Some(1), None)), (255, 69, 58));
        assert_eq!(status_rgb(&e(CommandStatus::LaunchFailed, None, None)), (255, 69, 58));
        assert_eq!(trigger_rgb(CommandTrigger::AutoReset), (191, 90, 242));
    }

    #[test]
    fn account_labels() {
        let names: HashMap<String, String> = [("a".to_string(), "work".to_string())].into();
        assert_eq!(account_label(Some("a"), &names), "work");
        assert_eq!(account_label(Some("gone"), &names), "Deleted account");
        assert_eq!(account_label(None, &names), "—");
    }
}
