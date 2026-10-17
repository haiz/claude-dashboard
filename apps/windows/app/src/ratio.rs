//! X, the 7d cost of a full 5h window (macOS `UsageRatioController`): recomputed at launch
//! and every hour from the usage log into `ratio_logs.db`; the latest value feeds the
//! critical assessment and the Dashboard header button.

use std::sync::Mutex;

use claude_dashboard_core::usage_ratio::RatioValue;

static CURRENT: Mutex<RatioValue> = Mutex::new(RatioValue::DEFAULT);

/// The X the app uses now.
pub fn current() -> RatioValue {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_current(v: RatioValue) {
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = v;
}
