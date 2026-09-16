//! Writes `usage-log.json`. Encoding: `contract/linux-usage-log.md`.
//! Compression policy: `contract/usage-log.md`, which applies unchanged and
//! runs BEFORE each insert.
//!
//! `apps/linux/lib/usageLog.js` remains the reference implementation and the
//! only reader; this exists because after the process split the daemon is the
//! only process running continuously, so it must be the writer.

use serde::{Deserialize, Serialize};

const FORMAT_VERSION: u64 = 1;

/// Window ids, matching `WINDOW` in `apps/linux/lib/usageLog.js`. `2` is the
/// retired Sonnet window: inert history, never written again.
pub const WINDOW_FIVE_HOUR: i64 = 0;
pub const WINDOW_SEVEN_DAY: i64 = 1;
pub const WINDOW_FABLE: i64 = 3;

pub fn usage_log_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join(".local")
                .join("share")
        });
    base.join("claude-dashboard").join("usage-log.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Row {
    pub id: u64,
    pub aid: String,
    pub w: i64,
    pub rat: i64,
    pub t: i64,
    pub u: i64,
    pub lim: i64,
}

#[derive(Serialize, Deserialize)]
struct Document {
    version: u64,
    rows: Vec<Row>,
}

/// Unix seconds truncated toward zero, matching `Int64(someDouble)` in Swift
/// and `Math.trunc` in JS.
pub fn to_unix_seconds(ms: i64) -> i64 {
    ms / 1000
}

/// `round(utilization * 100)` rounding half **away from zero**, which is what
/// Swift's `round()` does. Rust's `f64::round` has the same behaviour, so no
/// sign handling is needed — unlike JS, where `Math.round` rounds half toward
/// positive infinity and the reference implementation compensates.
pub fn encode_utilization(utilization: f64) -> i64 {
    (utilization * 100.0).round() as i64
}

#[derive(Default)]
pub struct UsageLog {
    rows: Vec<Row>,
    next_id: u64,
}

impl UsageLog {
    pub fn new() -> Self {
        Self { rows: Vec::new(), next_id: 1 }
    }

    pub fn from_json(text: &str) -> Self {
        let Ok(doc) = serde_json::from_str::<Document>(text) else {
            return Self::new();
        };
        if doc.version != FORMAT_VERSION {
            return Self::new();
        }
        let next_id = doc.rows.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        Self { rows: doc.rows, next_id }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn record(
        &mut self,
        aid: &str,
        w: i64,
        resets_at_ms: i64,
        utilization: f64,
        is_limited: bool,
        recorded_at_ms: i64,
    ) {
        let rat = to_unix_seconds(resets_at_ms);
        let t = to_unix_seconds(recorded_at_ms);
        let u = encode_utilization(utilization);

        self.apply_compression(aid, w, rat, u);

        let id = self.next_id;
        self.next_id += 1;
        self.rows.push(Row { id, aid: aid.to_string(), w, rat, t, u, lim: i64::from(is_limited) });
    }

    /// Run-length "keep first and last of a plateau", scoped to an exact
    /// (aid, w, rat) triple. All three conditions must hold or nothing is
    /// deleted: at least two existing rows for that exact triple, the two
    /// most recent by `t` (ties broken by `id`, both descending — matching
    /// `apps/linux/lib/usageLog.js`'s `_applyCompression` and
    /// `contract/usage-log.md`'s "Query the two most recent existing rows
    /// ... ordered by `t DESC LIMIT 2`") have equal `u` values, and that `u`
    /// equals the incoming `u`. Insertion order is only a proxy for `t DESC`
    /// and diverges from it whenever the clock steps backwards between two
    /// recordings of the same triple, so the matching set is sorted
    /// explicitly rather than read off array position.
    fn apply_compression(&mut self, aid: &str, w: i64, rat: i64, u: i64) {
        let mut matching: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.aid == aid && r.w == w && r.rat == rat)
            .map(|(i, _)| i)
            .collect();
        if matching.len() < 2 {
            return;
        }
        // t DESC, id DESC — contract/usage-log.md step 1.
        matching.sort_by(|&a, &b| {
            (self.rows[b].t, self.rows[b].id).cmp(&(self.rows[a].t, self.rows[a].id))
        });
        let (newest, second) = (matching[0], matching[1]);
        if self.rows[newest].u == u && self.rows[second].u == u {
            // Delete the more recent of the two: once the incoming row is
            // appended, that one becomes the middle of the run.
            self.rows.remove(newest);
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&Document { version: FORMAT_VERSION, rows: self.rows.clone() })
            .expect("a usage log always serializes")
    }
}

/// Same temp-file-then-`rename` discipline as `state::write_atomic`: the GTK
/// app reads this file while the daemon rewrites it, so a partial document
/// must never be observable.
pub fn write_string_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock step backwards within one `(aid, w, rat)` triple must still
    /// compress by `t DESC` (ties by `id DESC`), not by insertion order.
    /// Recordings arrive t=300, t=100, t=200, all at the same utilization —
    /// the two most recent BY TIME at the moment of the third insert are
    /// t=300 and t=100 (id 1 and id 2), so id 1 (the newer of those two) is
    /// the one deleted, leaving id 2 and the newly inserted id 3.
    ///
    /// The old, buggy "last two by array position" reading would instead see
    /// id 1 and id 2 (insertion order) as the pair to compare, delete id 2,
    /// and leave id 1 — the wrong row survives. Traced against
    /// `apps/linux/lib/usageLog.js`'s `_applyCompression` by hand:
    /// JS keeps rows 2 and 3.
    #[test]
    fn compression_orders_by_recorded_time_not_insertion_order() {
        let mut log = UsageLog::new();
        let rat_ms = 5_000_000_000_i64;
        log.record("acc-1", WINDOW_FIVE_HOUR, rat_ms, 0.5, false, 300_000);
        log.record("acc-1", WINDOW_FIVE_HOUR, rat_ms, 0.5, false, 100_000);
        log.record("acc-1", WINDOW_FIVE_HOUR, rat_ms, 0.5, false, 200_000);

        let survivors: Vec<(u64, i64)> = log.rows().iter().map(|r| (r.id, r.t)).collect();
        assert_eq!(survivors, vec![(2, 100), (3, 200)]);
    }

    /// A non-monotone `t` sequence that flips compress-vs-no-compress
    /// entirely, not just which row survives: (t=100,u=.5), (t=300,u=.5),
    /// (t=50,u=.7), (t=200,u=.5) — same triple throughout.
    ///
    /// By `t DESC`, the third insert's two most recent existing rows are
    /// id 2 (t=300) and id 1 (t=100), both u=.5, but the incoming u=.7
    /// mismatches, so nothing compresses. The fourth insert's two most
    /// recent existing rows are again id 2 (t=300) and id 1 (t=100) — id 3
    /// (t=50) is now the OLDEST, not the newest, despite being inserted
    /// last — both u=.5, and the incoming u=.5 matches, so id 2 is deleted.
    /// Final: 3 rows (ids 1, 3, 4), id 2 gone.
    ///
    /// The old "last two by array position" reading would instead compare
    /// id 3 (u=.7) against id 2 (u=.5) at both the third and fourth insert,
    /// see a mismatch every time, and never compress at all — 4 rows,
    /// nothing deleted.
    #[test]
    fn a_backwards_clock_step_can_flip_compress_vs_no_compress() {
        let mut log = UsageLog::new();
        let rat_ms = 5_000_000_000_i64;
        log.record("acc-1", WINDOW_FIVE_HOUR, rat_ms, 0.5, false, 100_000);
        log.record("acc-1", WINDOW_FIVE_HOUR, rat_ms, 0.5, false, 300_000);
        log.record("acc-1", WINDOW_FIVE_HOUR, rat_ms, 0.7, false, 50_000);
        log.record("acc-1", WINDOW_FIVE_HOUR, rat_ms, 0.5, false, 200_000);

        let survivors: Vec<(u64, i64, i64)> =
            log.rows().iter().map(|r| (r.id, r.t, r.u)).collect();
        assert_eq!(survivors, vec![(1, 100, 50), (3, 50, 70), (4, 200, 50)]);
    }
}
