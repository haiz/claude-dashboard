//! X: the share of the 7d quota (in %) that one full 5h window costs, for all accounts
//! together, and its history in `ratio_logs.db`. Port of macOS `UsageRatioEstimator`,
//! `UsageRatioStore` and `UsageRatioController.recompute`
//! (`docs/superpowers/specs/2026-10-16-critical-accounts-design.md`, "Estimate").

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection};

use crate::store::StoreError;

pub const LOOKBACK_S: f64 = 3.0 * 86_400.0;
/// How far back 7d rows are read beyond `LOOKBACK_S`: compression can leave the last row of
/// an unchanged 7d value days before the 5h window that needs it.
pub const SEVEN_DAY_LOOKBACK_S: f64 = 7.0 * 86_400.0;
pub const MIN_WINDOW_DELTA: f64 = 10.0;
pub const MIN_TOTAL_FIVE_HOUR: f64 = 100.0;
pub const DEFAULT_X: f64 = 16.0;
/// `resetsAt` drifts by a second or more between fetches; a real reset moves it by hours.
pub const RESET_TOLERANCE_S: f64 = 60.0;
/// The 5h and 7d rows of one refresh are written separately and can land a second apart.
pub const SAMPLE_TOLERANCE_S: f64 = 5.0;
pub const INTERVAL_S: u64 = 3600;
pub const RETENTION_S: f64 = 90.0 * 86_400.0;

/// One `usage_logs` row as the estimator reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRow {
    pub account_id: String,
    pub t_unix: f64,
    pub utilization: f64,
    pub resets_at_unix: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub x: f64,
    pub sum_five_hour: f64,
    pub sum_seven_day: f64,
}

pub fn estimate(five_hour: &[LogRow], seven_day: &[LogRow]) -> Option<Estimate> {
    let mut weekly: HashMap<&str, Vec<&LogRow>> = HashMap::new();
    for r in seven_day {
        weekly.entry(r.account_id.as_str()).or_default().push(r);
    }
    for series in weekly.values_mut() {
        series.sort_by(|a, b| a.t_unix.total_cmp(&b.t_unix));
    }
    let (mut sum_five, mut sum_seven) = (0.0, 0.0);
    for window in windows(five_hour) {
        let (Some(first), Some(last)) = (window.first(), window.last()) else { continue };
        if last.utilization - first.utilization < MIN_WINDOW_DELTA {
            continue;
        }
        let Some(series) = weekly.get(first.account_id.as_str()) else { continue };
        let (Some(w0), Some(w1)) = (reading(series, first.t_unix), reading(series, last.t_unix)) else { continue };
        // A week reset inside the window, or 7d fell without one (a correction, not a cost).
        if (w1.resets_at_unix - w0.resets_at_unix).abs() > RESET_TOLERANCE_S || w1.utilization - w0.utilization < 0.0 {
            continue;
        }
        sum_five += last.utilization - first.utilization;
        sum_seven += w1.utilization - w0.utilization;
    }
    (sum_five >= MIN_TOTAL_FIVE_HOUR).then(|| Estimate { x: 100.0 * sum_seven / sum_five, sum_five_hour: sum_five, sum_seven_day: sum_seven })
}

/// Per account, in time order; a `resetsAt` more than `RESET_TOLERANCE_S` away from the
/// current window's first row starts the next window.
fn windows(rows: &[LogRow]) -> Vec<Vec<&LogRow>> {
    let mut by_account: HashMap<&str, Vec<&LogRow>> = HashMap::new();
    for r in rows {
        by_account.entry(r.account_id.as_str()).or_default().push(r);
    }
    let mut result = Vec::new();
    for (_, mut account_rows) in by_account {
        account_rows.sort_by(|a, b| a.t_unix.total_cmp(&b.t_unix));
        let mut current: Vec<&LogRow> = Vec::new();
        for r in account_rows {
            if current.first().is_some_and(|anchor| (r.resets_at_unix - anchor.resets_at_unix).abs() > RESET_TOLERANCE_S) {
                result.push(std::mem::take(&mut current));
            }
            current.push(r);
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    result
}

/// The last 7d row at or before `t` (plus the sample tolerance).
fn reading<'a>(series: &[&'a LogRow], t: f64) -> Option<&'a LogRow> {
    series.iter().rev().find(|r| r.t_unix <= t + SAMPLE_TOLERANCE_S).copied()
}

/// One hourly measurement of X.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatioPoint {
    pub t_unix: f64,
    pub x: f64,
    pub sum_five_hour: f64,
    pub sum_seven_day: f64,
}

/// The X the app uses now. `updated_unix == None` means no measurement yet: `x` is the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatioValue {
    pub x: f64,
    pub updated_unix: Option<f64>,
}

impl RatioValue {
    pub const DEFAULT: RatioValue = RatioValue { x: DEFAULT_X, updated_unix: None };

    pub fn is_default(&self) -> bool {
        self.updated_unix.is_none()
    }
}

/// History of X in `ratio_logs.db`. Times are stored in whole seconds.
pub struct RatioStore {
    conn: Connection,
}

impl RatioStore {
    pub fn open() -> Result<Self, StoreError> {
        Self::open_at(&crate::store::ratio_log_path())
    }

    pub fn open_at(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ratio_logs (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 t INTEGER NOT NULL, x REAL NOT NULL, d5 REAL NOT NULL, d7 REAL NOT NULL);
             CREATE INDEX IF NOT EXISTS idx_ratio_t ON ratio_logs(t);",
        )?;
        Ok(RatioStore { conn })
    }

    /// Best-effort, like the other logs: a SQLite failure drops the point.
    pub fn insert(&self, p: &RatioPoint) {
        let _ = self.conn.execute(
            "INSERT INTO ratio_logs (t, x, d5, d7) VALUES (?1, ?2, ?3, ?4)",
            params![p.t_unix as i64, p.x, p.sum_five_hour, p.sum_seven_day],
        );
    }

    pub fn latest(&self) -> Option<RatioPoint> {
        self.query("SELECT t, x, d5, d7 FROM ratio_logs ORDER BY t DESC, id DESC LIMIT 1", &[]).into_iter().next()
    }

    /// Oldest first; `None` bounds are open.
    pub fn series(&self, from_unix: Option<f64>, to_unix: Option<f64>) -> Vec<RatioPoint> {
        let mut sql = String::from("SELECT t, x, d5, d7 FROM ratio_logs WHERE 1 = 1");
        let mut binds = Vec::new();
        if let Some(f) = from_unix {
            sql.push_str(" AND t >= ?");
            binds.push(f as i64);
        }
        if let Some(t) = to_unix {
            sql.push_str(" AND t <= ?");
            binds.push(t as i64);
        }
        sql.push_str(" ORDER BY t ASC, id ASC");
        self.query(&sql, &binds)
    }

    pub fn delete_older_than(&self, cutoff_unix: f64) {
        let _ = self.conn.execute("DELETE FROM ratio_logs WHERE t < ?1", params![cutoff_unix as i64]);
    }

    fn query(&self, sql: &str, binds: &[i64]) -> Vec<RatioPoint> {
        let run = || -> rusqlite::Result<Vec<RatioPoint>> {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(binds.iter()), |r| {
                Ok(RatioPoint {
                    t_unix: r.get::<_, i64>(0)? as f64,
                    x: r.get(1)?,
                    sum_five_hour: r.get(2)?,
                    sum_seven_day: r.get(3)?,
                })
            })?;
            rows.collect()
        };
        run().unwrap_or_default()
    }
}

/// One hourly run (macOS `UsageRatioController.recompute`): estimates X from the rows
/// `fetch(five_from, seven_from)` returns, stores a non-nil estimate, drops history older
/// than 90 days and returns the latest stored value (the default when there is none).
pub fn recompute(store: &RatioStore, fetch: impl FnOnce(f64, f64) -> (Vec<LogRow>, Vec<LogRow>), now_unix: f64) -> RatioValue {
    let lookback_start = now_unix - LOOKBACK_S;
    let (five, seven) = fetch(lookback_start, lookback_start - SEVEN_DAY_LOOKBACK_S);
    if let Some(e) = estimate(&five, &seven) {
        store.insert(&RatioPoint { t_unix: now_unix, x: e.x, sum_five_hour: e.sum_five_hour, sum_seven_day: e.sum_seven_day });
    }
    store.delete_older_than(now_unix - RETENTION_S);
    store
        .latest()
        .map_or(RatioValue::DEFAULT, |p| RatioValue { x: p.x, updated_unix: Some(p.t_unix) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: f64 = 1_800_000_000.0;

    fn row(account: &str, minutes: f64, resets: f64, u: f64) -> LogRow {
        LogRow { account_id: account.into(), t_unix: T0 + minutes * 60.0, utilization: u, resets_at_unix: T0 + resets }
    }

    fn est(x: f64, five: f64, seven: f64) -> Option<Estimate> {
        Some(Estimate { x, sum_five_hour: five, sum_seven_day: seven })
    }

    #[test]
    fn single_window() {
        let five = [row("a", 0.0, 18_000.0, 0.0), row("a", 290.0, 18_000.0, 100.0)];
        let seven = [row("a", 0.0, 500_000.0, 10.0), row("a", 290.0, 500_000.0, 26.0)];
        assert_eq!(estimate(&five, &seven), est(16.0, 100.0, 16.0));
    }

    #[test]
    fn two_accounts_are_pooled() {
        let five = [
            row("a", 0.0, 18_000.0, 0.0), row("a", 100.0, 18_000.0, 60.0),
            row("b", 0.0, 19_000.0, 20.0), row("b", 100.0, 19_000.0, 60.0),
        ];
        let seven = [
            row("a", 0.0, 500_000.0, 10.0), row("a", 100.0, 500_000.0, 20.0),
            row("b", 0.0, 600_000.0, 40.0), row("b", 100.0, 600_000.0, 46.0),
        ];
        let e = estimate(&five, &seven).unwrap();
        assert_eq!((e.sum_five_hour, e.sum_seven_day), (100.0, 16.0));
        assert!((e.x - 16.0).abs() < 1e-9);
    }

    #[test]
    fn jittered_resets_at_is_one_window() {
        let five = [row("a", 0.0, 17_999.0, 0.0), row("a", 100.0, 18_000.0, 50.0), row("a", 200.0, 17_999.0, 100.0)];
        let seven = [row("a", 0.0, 500_000.0, 10.0), row("a", 200.0, 500_001.0, 26.0)];
        assert_eq!(estimate(&five, &seven).map(|e| e.sum_five_hour), Some(100.0));
    }

    #[test]
    fn window_with_weekly_reset_inside_is_skipped() {
        let five = [row("a", 0.0, 18_000.0, 0.0), row("a", 290.0, 18_000.0, 100.0)];
        let seven = [row("a", 0.0, 1_000.0, 90.0), row("a", 290.0, 600_000.0, 5.0)];
        assert_eq!(estimate(&five, &seven), None);
    }

    #[test]
    fn small_windows_are_skipped() {
        let mut five = Vec::new();
        for i in 0..20 {
            let resets = i as f64 * 18_000.0 + 18_000.0;
            five.push(row("a", i as f64 * 300.0, resets, 0.0));
            five.push(row("a", i as f64 * 300.0 + 100.0, resets, 9.0));
        }
        let seven = [row("a", 0.0, 900_000.0, 10.0)];
        assert_eq!(estimate(&five, &seven), None);
    }

    #[test]
    fn not_enough_data_is_none() {
        let five = [row("a", 0.0, 18_000.0, 0.0), row("a", 100.0, 18_000.0, 99.0)];
        let seven = [row("a", 0.0, 500_000.0, 10.0), row("a", 100.0, 500_000.0, 26.0)];
        assert_eq!(estimate(&five, &seven), None);
    }

    #[test]
    fn sparse_seven_day_rows_and_sample_tolerance() {
        let five = [row("a", 600.0, 36_000.0, 0.0), row("a", 890.0, 36_000.0, 100.0)];
        let seven = [row("a", 0.0, 500_000.0, 10.0), row("a", 890.05, 500_000.0, 26.0)];
        assert!((estimate(&five, &seven).unwrap().x - 16.0).abs() < 1e-9);
    }

    #[test]
    fn missing_seven_day_is_skipped() {
        let five = [row("a", 0.0, 18_000.0, 0.0), row("a", 290.0, 18_000.0, 100.0)];
        assert_eq!(estimate(&five, &[]), None);
    }

    #[test]
    fn window_where_seven_day_dropped_is_skipped() {
        let mut five = vec![
            row("a", 0.0, 18_000.0, 0.0), row("a", 290.0, 18_000.0, 100.0),
            row("b", 0.0, 19_000.0, 0.0), row("b", 290.0, 19_000.0, 100.0),
        ];
        let mut seven = vec![
            row("a", 0.0, 500_000.0, 10.0), row("a", 290.0, 500_000.0, 26.0),
            row("b", 0.0, 600_000.0, 50.0), row("b", 290.0, 600_000.0, 30.0),
        ];
        assert_eq!(estimate(&five, &seven), est(16.0, 100.0, 16.0), "only A's window counts");
        five.extend([row("b", 600.0, 40_000.0, 0.0), row("b", 890.0, 40_000.0, 100.0)]);
        seven.push(row("b", 890.0, 600_000.0, 46.0));
        assert_eq!(estimate(&five, &seven), est(16.0, 200.0, 32.0));
    }

    fn point(t: f64, x: f64) -> RatioPoint {
        RatioPoint { t_unix: t, x, sum_five_hour: 100.0, sum_seven_day: x }
    }

    #[test]
    fn store_insert_latest_series_and_retention() {
        let s = RatioStore::open_in_memory().unwrap();
        assert_eq!(s.latest(), None);
        s.insert(&point(T0, 15.0));
        s.insert(&point(T0 + 3600.4, 16.5));
        s.insert(&point(T0 + 7200.0, 17.0));
        assert_eq!(s.latest(), Some(point(T0 + 7200.0, 17.0)));
        assert_eq!(s.series(None, None).iter().map(|p| p.x).collect::<Vec<_>>(), vec![15.0, 16.5, 17.0]);
        assert_eq!(s.series(None, None)[1].t_unix, T0 + 3600.0, "whole seconds");
        assert_eq!(s.series(Some(T0 + 1.0), None).len(), 2);
        assert_eq!(s.series(None, Some(T0 + 3600.0)).len(), 2);
        s.delete_older_than(T0 + 3600.0);
        assert_eq!(s.series(None, None).iter().map(|p| p.x).collect::<Vec<_>>(), vec![16.5, 17.0]);
    }

    #[test]
    fn store_on_disk_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested").join("ratio_logs.db");
        RatioStore::open_at(&p).unwrap().insert(&point(T0, 15.0));
        assert_eq!(RatioStore::open_at(&p).unwrap().latest().map(|p| p.x), Some(15.0));
    }

    /// Rows for one 5h window ending a minute before `now`.
    fn rows(now: f64, five_per_window: f64, seven_per_window: f64) -> (Vec<LogRow>, Vec<LogRow>) {
        let r = |t: f64, resets: f64, u: f64| LogRow { account_id: "a".into(), t_unix: t, utilization: u, resets_at_unix: resets };
        (
            vec![r(now - 7200.0, now, 0.0), r(now - 60.0, now, five_per_window)],
            vec![r(now - 7200.0, now + 86_400.0, 10.0), r(now - 60.0, now + 86_400.0, 10.0 + seven_per_window)],
        )
    }

    #[test]
    fn recompute_stores_and_publishes() {
        let s = RatioStore::open_in_memory().unwrap();
        let v = recompute(&s, |_, _| rows(T0, 100.0, 17.0), T0);
        assert_eq!(v, RatioValue { x: 17.0, updated_unix: Some(T0) });
        assert_eq!(s.series(None, None).len(), 1);
    }

    #[test]
    fn recompute_with_empty_logs_stays_at_the_default() {
        let s = RatioStore::open_in_memory().unwrap();
        let v = recompute(&s, |_, _| (Vec::new(), Vec::new()), T0);
        assert_eq!(v, RatioValue::DEFAULT);
        assert!(v.is_default());
    }

    #[test]
    fn not_enough_data_keeps_the_latest_stored_value() {
        let s = RatioStore::open_in_memory().unwrap();
        s.insert(&RatioPoint { t_unix: T0 - 3600.0, x: 15.5, sum_five_hour: 300.0, sum_seven_day: 46.5 });
        let v = recompute(&s, |_, _| rows(T0, 50.0, 8.0), T0);
        assert_eq!(v, RatioValue { x: 15.5, updated_unix: Some(T0 - 3600.0) });
        assert_eq!(s.series(None, None).len(), 1);
    }

    #[test]
    fn recompute_drops_history_older_than_90_days() {
        let s = RatioStore::open_in_memory().unwrap();
        s.insert(&point(T0 - 91.0 * 86_400.0, 12.0));
        recompute(&s, |_, _| rows(T0, 100.0, 16.0), T0);
        assert_eq!(s.series(None, None).iter().map(|p| p.x).collect::<Vec<_>>(), vec![16.0]);
    }

    #[test]
    fn fetch_windows_are_lookback_and_lookback_plus_a_week() {
        let s = RatioStore::open_in_memory().unwrap();
        let mut asked = None;
        recompute(&s, |five, seven| { asked = Some((five, seven)); (Vec::new(), Vec::new()) }, T0);
        assert_eq!(asked, Some((T0 - 3.0 * 86_400.0, T0 - 10.0 * 86_400.0)));
    }
}
