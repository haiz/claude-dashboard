//! Display copy for limit resets (Rust mirror of macOS `LimitResetsText`): the
//! account page's detail line, the gauge badge's tooltip, the relative
//! "in N days" text, and [`view`], which builds all of them for one account.
//!
//! Pure: `now` and the local UTC offset (seconds, positive east; the same
//! shift `format_reset_time` callers apply) are passed in. The year is shown
//! only when the expiry is not in the current local year. Rules:
//! `contract/README.md` "Limit resets".
//!
//! ```
//! use claude_dashboard_core::limit_resets::{ResetKind, ResetSummary};
//! use claude_dashboard_core::limit_resets_text::{detail_line, tooltip};
//! let s = ResetSummary { count: 1, expires_at: 1_796_940_000, urgent: false };
//! let now = 1_794_441_600; // 2026-11-12T00:00:00Z
//! assert_eq!(
//!     detail_line(ResetKind::Full, &s, now, 0, 2026),
//!     ("Full reset · 1".to_string(), "Expires Dec 10, 22:00 (in 28 days)".to_string())
//! );
//! assert_eq!(tooltip(ResetKind::Full, &s, now, 0, 2026), "Full reset · expires Dec 10, 22:00");
//! ```

use crate::limit_resets::{summarize, ResetGrant, ResetKind, ResetSummary};

/// Title and expiry text of one "Limit resets" line: ("Full reset · 1",
/// "Expires Dec 10, 22:00 (in 28 days)"). The title is always singular.
pub fn detail_line(
    kind: ResetKind,
    s: &ResetSummary,
    now_unix: i64,
    local_offset_s: i64,
    current_year: i32,
) -> (String, String) {
    let when = expiry_date(s.expires_at, local_offset_s, current_year);
    let left = relative(s.expires_at - now_unix);
    (format!("{} reset · {}", name(kind), s.count), format!("Expires {when} ({left})"))
}

/// One reset: "Full reset · expires ...". Several: "Fable resets · 2 · expire ...".
/// `now_unix` is unused (the year comes from `current_year`); kept so the three
/// copy functions share one call shape.
pub fn tooltip(kind: ResetKind, s: &ResetSummary, _now_unix: i64, local_offset_s: i64, current_year: i32) -> String {
    let when = expiry_date(s.expires_at, local_offset_s, current_year);
    if s.count == 1 {
        format!("{} reset · expires {when}", name(kind))
    } else {
        format!("{} resets · {} · expire {when}", name(kind), s.count)
    }
}

/// `>= 1 day`: "in N days" ("in 1 day"); under a day: "in N h", N at least 1.
pub fn relative(seconds: i64) -> String {
    if seconds >= 86_400 {
        let days = seconds / 86_400;
        return if days == 1 { "in 1 day".into() } else { format!("in {days} days") };
    }
    format!("in {} h", (seconds / 3_600).max(1))
}

/// The local calendar year at `now_unix`.
pub fn current_year(now_unix: i64, local_offset_s: i64) -> i32 {
    use chrono::Datelike;
    local(now_unix, local_offset_s).year()
}

/// A gauge badge: the count, red when urgent, and its tooltip.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Badge {
    pub count: u32,
    pub urgent: bool,
    pub tip: String,
}

/// One line of the account page's "Limit resets" section.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Line {
    pub title: String,
    pub expiry: String,
    pub urgent: bool,
}

/// Everything the views show for one account's grants at `now_unix`: the 7d
/// badge (`full`), the F badge (`fable`) and the section lines, Full first.
/// Empty once every grant has ended, so re-running it on a timer is the
/// time-based refresh.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResetsView {
    pub full: Option<Badge>,
    pub fable: Option<Badge>,
    pub lines: Vec<Line>,
}

pub fn view(grants: &[ResetGrant], now_unix: i64, local_offset_s: i64) -> ResetsView {
    let resets = summarize(grants, now_unix);
    let year = current_year(now_unix, local_offset_s);
    let mut out = ResetsView::default();
    for (kind, summary, slot) in [
        (ResetKind::Full, &resets.full, &mut out.full),
        (ResetKind::Fable, &resets.fable, &mut out.fable),
    ] {
        let Some(s) = summary else { continue };
        *slot = Some(Badge { count: s.count, urgent: s.urgent, tip: tooltip(kind, s, now_unix, local_offset_s, year) });
        let (title, expiry) = detail_line(kind, s, now_unix, local_offset_s, year);
        out.lines.push(Line { title, expiry, urgent: s.urgent });
    }
    out
}

fn name(kind: ResetKind) -> &'static str {
    match kind {
        ResetKind::Full => "Full",
        ResetKind::Fable => "Fable",
    }
}

fn local(unix: i64, local_offset_s: i64) -> chrono::NaiveDateTime {
    chrono::DateTime::from_timestamp(unix.saturating_add(local_offset_s), 0)
        .unwrap_or_default()
        .naive_utc()
}

/// "Dec 10, 22:00", or "Mar 9, 2027, 14:59" outside `current_year`.
fn expiry_date(unix: i64, local_offset_s: i64, current_year: i32) -> String {
    use chrono::Datelike;
    let t = local(unix, local_offset_s);
    let fmt = if t.year() == current_year { "%b %-d, %H:%M" } else { "%b %-d, %Y, %H:%M" };
    t.format(fmt).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFSET: i64 = 25_200; // UTC+7
    const NOW: i64 = 1_794_441_600; // 2026-11-12T00:00:00Z
    const DEC_10_22_00Z: i64 = 1_796_940_000; // 2026-12-10T22:00:00Z
    const MAR_9_07_59Z: i64 = 1_804_579_140; // 2027-03-09T07:59:00Z

    fn full() -> ResetSummary {
        ResetSummary { count: 1, expires_at: DEC_10_22_00Z, urgent: false }
    }

    fn fable() -> ResetSummary {
        ResetSummary { count: 2, expires_at: MAR_9_07_59Z, urgent: false }
    }

    #[test]
    fn full_detail_line() {
        let (title, expiry) = detail_line(ResetKind::Full, &full(), NOW, OFFSET, 2026);
        assert_eq!(title, "Full reset · 1");
        assert_eq!(expiry, "Expires Dec 11, 05:00 (in 28 days)");
    }

    #[test]
    fn fable_detail_line_shows_year_when_not_current() {
        let (title, expiry) = detail_line(ResetKind::Fable, &fable(), NOW, OFFSET, 2026);
        assert_eq!(title, "Fable reset · 2");
        assert_eq!(expiry, "Expires Mar 9, 2027, 14:59 (in 117 days)");
    }

    #[test]
    fn utc_copy_matches_the_spec() {
        let (title, expiry) = detail_line(ResetKind::Full, &full(), NOW, 0, 2026);
        assert_eq!(format!("{title} \u{2014} {expiry}"), "Full reset · 1 \u{2014} Expires Dec 10, 22:00 (in 28 days)");
    }

    #[test]
    fn relative_text() {
        assert_eq!(relative(86_400), "in 1 day");
        assert_eq!(relative(86_399), "in 23 h");
        assert_eq!(relative(120), "in 1 h");
        assert_eq!(relative(28 * 86_400 + 79_200), "in 28 days");
        assert_eq!(relative(5 * 3_600 + 59), "in 5 h");
    }

    #[test]
    fn tooltip_plural_for_several() {
        assert_eq!(
            tooltip(ResetKind::Fable, &fable(), NOW, OFFSET, 2026),
            "Fable resets · 2 · expire Mar 9, 2027, 14:59"
        );
    }

    #[test]
    fn tooltip_singular_for_one() {
        assert_eq!(tooltip(ResetKind::Full, &full(), NOW, OFFSET, 2026), "Full reset · expires Dec 11, 05:00");
    }

    #[test]
    fn current_year_is_local() {
        // 2026-12-31T20:00:00Z is already 2027-01-01 03:00 at UTC+7.
        let eve = 1_798_747_200;
        assert_eq!(current_year(eve, 0), 2026);
        assert_eq!(current_year(eve, OFFSET), 2027);
    }

    #[test]
    fn view_builds_badges_and_lines_in_full_then_fable_order() {
        let grants = [
            ResetGrant { kind: ResetKind::Fable, resets_left: 2, ends_at: MAR_9_07_59Z },
            ResetGrant { kind: ResetKind::Full, resets_left: 1, ends_at: DEC_10_22_00Z },
        ];
        let v = view(&grants, NOW, OFFSET);
        let full = v.full.as_ref().unwrap();
        assert_eq!((full.count, full.urgent), (1, false));
        assert_eq!(full.tip, "Full reset · expires Dec 11, 05:00");
        let fable = v.fable.as_ref().unwrap();
        assert_eq!((fable.count, fable.urgent), (2, false));
        assert_eq!(fable.tip, "Fable resets · 2 · expire Mar 9, 2027, 14:59");
        assert_eq!(v.lines.len(), 2);
        assert_eq!(v.lines[0].title, "Full reset · 1");
        assert_eq!(v.lines[1].title, "Fable reset · 2");
        assert_eq!(v.lines[1].expiry, "Expires Mar 9, 2027, 14:59 (in 117 days)");
    }

    #[test]
    fn view_marks_urgent_and_switches_to_hours() {
        let six_days = [ResetGrant { kind: ResetKind::Full, resets_left: 1, ends_at: NOW + 6 * 86_400 }];
        let v = view(&six_days, NOW, 0);
        assert!(v.full.as_ref().unwrap().urgent);
        assert!(v.lines[0].urgent);
        let five_hours = [ResetGrant { kind: ResetKind::Full, resets_left: 1, ends_at: NOW + 5 * 3_600 }];
        assert_eq!(view(&five_hours, NOW, 0).lines[0].expiry, "Expires Nov 12, 05:00 (in 5 h)");
    }

    #[test]
    fn view_after_expiry_is_empty() {
        let grants = [ResetGrant { kind: ResetKind::Full, resets_left: 1, ends_at: DEC_10_22_00Z }];
        let v = view(&grants, DEC_10_22_00Z, OFFSET);
        assert!(v.full.is_none() && v.fable.is_none() && v.lines.is_empty());
        assert_eq!(view(&[], NOW, OFFSET), ResetsView::default());
    }
}
