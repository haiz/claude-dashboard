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
    /// deleted: at least two existing rows for that exact triple, their last
    /// two `u` values equal to each other, and equal to the incoming `u`.
    fn apply_compression(&mut self, aid: &str, w: i64, rat: i64, u: i64) {
        let matching: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.aid == aid && r.w == w && r.rat == rat)
            .map(|(i, _)| i)
            .collect();
        if matching.len() < 2 {
            return;
        }
        let last = matching[matching.len() - 1];
        let prev = matching[matching.len() - 2];
        if self.rows[last].u == u && self.rows[prev].u == u {
            self.rows.remove(last);
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
