//! Tools > Ping Log (macOS `PingLogView` / `PingLogViewModel`): every ping, newest first.

use std::collections::HashMap;

use claude_dashboard_core::ping::PingOutcome;
use claude_dashboard_core::ping_log::PingLogEntry;

use crate::UiPingRow;

/// The rows for `entries` (newest first). `names` maps account ids to names; an id not in
/// it reads "Removed account". Times are local, shifted by `local_offset_s`.
pub fn rows(entries: &[PingLogEntry], names: &HashMap<String, String>, local_offset_s: f64) -> Vec<UiPingRow> {
    entries
        .iter()
        .map(|e| UiPingRow {
            time: medium_date_time(e.started_unix + local_offset_s).into(),
            trigger: e.trigger.label().into(),
            account: names.get(&e.account_id).map_or("Removed account", String::as_str).into(),
            outcome: e.outcome.label().into(),
            ok: e.outcome == PingOutcome::Ok,
            duration: format!("{:.1}s", (e.finished_unix - e.started_unix).max(0.0)).into(),
            detail: e.detail.clone().unwrap_or_default().into(),
        })
        .collect()
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// macOS `.medium` date and time ("Oct 17, 2026 at 10:44:03 AM") for local seconds.
pub fn medium_date_time(local_unix: f64) -> String {
    let secs = local_unix.floor() as i64;
    let (days, day_s) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    let (h, min, s) = (day_s / 3600, (day_s % 3600) / 60, day_s % 60);
    let h12 = if h % 12 == 0 { 12 } else { h % 12 };
    let ampm = if h < 12 { "AM" } else { "PM" };
    format!("{} {d}, {y} at {h12}:{min:02}:{s:02} {ampm}", MONTHS[(m - 1) as usize])
}

/// macOS `.abbreviated` date with `.shortened` time ("Oct 17, 2026 at 10:44 AM").
pub fn abbreviated_date_time(local_unix: f64) -> String {
    let full = medium_date_time(local_unix);
    // Drop the seconds: "10:44:03 AM" -> "10:44 AM".
    match (full.rfind(':'), full.rfind(' ')) {
        (Some(c), Some(sp)) if c < sp => format!("{}{}", &full[..c], &full[sp..]),
        _ => full,
    }
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar
/// (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::ping_log::PingTrigger;

    fn e(account: &str, outcome: PingOutcome, detail: Option<&str>) -> PingLogEntry {
        PingLogEntry {
            id: 1,
            account_id: account.into(),
            trigger: PingTrigger::Auto,
            // 2026-10-17 03:44:03 UTC
            started_unix: 1_792_208_643.0,
            finished_unix: 1_792_208_644.25,
            outcome,
            detail: detail.map(String::from),
        }
    }

    #[test]
    fn rows_format_time_name_and_duration() {
        let names = HashMap::from([("a".to_string(), "alice".to_string())]);
        let r = &rows(&[e("a", PingOutcome::Ok, None)], &names, 7.0 * 3600.0)[0];
        assert_eq!(r.time.as_str(), "Oct 17, 2026 at 10:44:03 AM");
        assert_eq!(r.trigger.as_str(), "Auto");
        assert_eq!(r.account.as_str(), "alice");
        assert_eq!(r.outcome.as_str(), "OK");
        assert!(r.ok);
        assert_eq!(r.duration.as_str(), "1.2s");
        assert_eq!(r.detail.as_str(), "");
    }

    #[test]
    fn removed_account_name_and_failure() {
        let r = &rows(&[e("gone", PingOutcome::Failed, Some("HTTP 429"))], &HashMap::new(), 0.0)[0];
        assert_eq!(r.account.as_str(), "Removed account");
        assert_eq!(r.outcome.as_str(), "Failed");
        assert!(!r.ok);
        assert_eq!(r.detail.as_str(), "HTTP 429");
        assert_eq!(r.time.as_str(), "Oct 17, 2026 at 3:44:03 AM");
    }

    #[test]
    fn clear_forgets_latest() {
        let mut c = crate::ping::PingController::default();
        assert!(c.begin("a"));
        c.finish(e("a", PingOutcome::Ok, None), 0.0);
        assert!(c.latest("a").is_some());
        c.clear_latest();
        assert!(c.latest("a").is_none());
    }
}
