//! Tools > Switch Log (macOS `AutoSwitchLogView` / `AutoSwitchLogViewModel`): every auto
//! switch, failed attempt and no-candidate stretch, and every Switch button press, newest
//! first, each tagged Auto or Manual.

use claude_dashboard_core::auto_switch_log::{Entry, Outcome, SwitchTarget};

use crate::UiSwitchRow;

/// The rows for `entries` (newest first). Times are local, shifted by `local_offset_s`.
pub fn rows(entries: &[Entry], local_offset_s: f64) -> Vec<UiSwitchRow> {
    entries
        .iter()
        .map(|e| UiSwitchRow {
            time: crate::ping_log_view::medium_date_time(e.date_unix + local_offset_s).into(),
            trigger: e.trigger.label().into(),
            accounts: accounts(e.from_name.as_deref(), e.to_name.as_deref(), e.target == SwitchTarget::ClaudeDesktop).into(),
            desktop: e.target == SwitchTarget::ClaudeDesktop,
            reason: e.reason.as_str().into(),
            detail: e.detail.clone().unwrap_or_default().into(),
            outcome: e.outcome.label().into(),
            kind: match e.outcome {
                Outcome::Switched => 0,
                Outcome::Failed => 1,
                Outcome::NoCandidate | Outcome::Skipped => 2,
            },
        })
        .collect()
}

/// "from → to", or just "from" when nothing was switched to (macOS `accounts(_:)`). A Claude
/// Desktop row does not record the account Desktop was on, so it reads "→ to".
pub fn accounts(from: Option<&str>, to: Option<&str>, desktop: bool) -> String {
    if desktop && from.is_none() {
        if let Some(to) = to {
            return format!("\u{2192} {to}");
        }
    }
    let from = from.unwrap_or("Unknown account");
    match to {
        Some(to) => format!("{from} \u{2192} {to}"),
        None => from.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::auto_switch_log::{SwitchTarget, Trigger};

    fn e(outcome: Outcome, from: Option<&str>, to: Option<&str>, detail: Option<&str>) -> Entry {
        Entry {
            id: 1,
            // 2026-10-17 03:44:03 UTC
            date_unix: 1_792_208_643.0,
            trigger: Trigger::Auto,
            target: SwitchTarget::ClaudeCode,
            outcome,
            from_name: from.map(Into::into),
            to_name: to.map(Into::into),
            reason: "5h 96% >= 95%, tier 1".into(),
            detail: detail.map(Into::into),
        }
    }

    #[test]
    fn rows_format_time_accounts_and_outcome() {
        let r = &rows(&[e(Outcome::Switched, Some("a@x.com"), Some("b@x.com"), Some("b@x.com: 5h 10%, 7d 20%"))], 7.0 * 3600.0)[0];
        assert_eq!(r.time.as_str(), "Oct 17, 2026 at 10:44:03 AM");
        assert_eq!(r.accounts.as_str(), "a@x.com \u{2192} b@x.com");
        assert_eq!(r.reason.as_str(), "5h 96% >= 95%, tier 1");
        assert_eq!(r.detail.as_str(), "b@x.com: 5h 10%, 7d 20%");
        assert_eq!((r.outcome.as_str(), r.kind), ("Switched", 0));
        assert_eq!(r.trigger.as_str(), "Auto");
        let manual = &rows(&[Entry { trigger: Trigger::Manual, ..e(Outcome::Switched, Some("a"), Some("b"), None) }], 0.0)[0];
        assert_eq!(manual.trigger.as_str(), "Manual");
        let failed = &rows(&[e(Outcome::Failed, None, Some("b"), None)], 0.0)[0];
        assert_eq!((failed.outcome.as_str(), failed.kind, failed.detail.as_str()), ("Failed", 1, ""));
        let none = &rows(&[e(Outcome::NoCandidate, Some("a"), None, None)], 0.0)[0];
        assert_eq!((none.outcome.as_str(), none.kind, none.accounts.as_str()), ("No candidate", 2, "a"));
    }

    #[test]
    fn desktop_row_has_badge_and_arrow() {
        let code = &rows(&[e(Outcome::Switched, Some("a"), Some("b"), None)], 0.0)[0];
        assert!(!code.desktop, "Claude Code rows carry no badge");
        let desktop = Entry { target: SwitchTarget::ClaudeDesktop, ..e(Outcome::Switched, None, Some("b@x.com"), None) };
        let r = &rows(&[desktop], 0.0)[0];
        assert!(r.desktop);
        assert_eq!(r.accounts.as_str(), "\u{2192} b@x.com");
        assert_eq!(accounts(Some("a"), Some("b"), true), "a \u{2192} b");
        assert_eq!(accounts(None, None, true), "Unknown account");
    }

    #[test]
    fn skipped_outcome_label() {
        let r = &rows(&[e(Outcome::Skipped, None, Some("b"), Some("no captured login"))], 0.0)[0];
        assert_eq!((r.outcome.as_str(), r.kind), ("Skipped", 2));
    }

    #[test]
    fn accounts_text() {
        assert_eq!(accounts(None, Some("b"), false), "Unknown account \u{2192} b");
        assert_eq!(accounts(None, None, false), "Unknown account");
    }
}
