//! InfoTip texts for Settings > General, copied from macOS `SettingsTips.swift`
//! (every Settings > General tip; "menu bar" reads "tray flyout" and "the Mac's" "the PC's").

pub struct Tip {
    pub id: &'static str,
    pub summary: &'static str,
    pub detail: &'static str,
    pub example: &'static str,
    /// A `help::TOPICS` id.
    pub help_topic: &'static str,
}

pub const TIPS: &[Tip] = &[
    Tip {
        id: "auto-refresh",
        summary: "Fetch usage for every account on a timer.",
        detail: "When on, the app fetches usage for all accounts every interval. When off, usage only updates when you press Refresh (auto switch still polls the active account near a limit).",
        example: "With a 5 min interval, a card that shows 40% at 10:00 shows the new figure by 10:05.",
        help_topic: "refresh-updates",
    },
    Tip {
        id: "refresh-interval",
        summary: "Minutes between automatic refreshes (1 to 60).",
        detail: "Shorter intervals keep cards fresher and let auto switch react sooner, at the cost of more requests to claude.ai.",
        example: "At 1 min, auto switch sees a 5h window reach 95% within a minute; at 15 min it can be 15 minutes late.",
        help_topic: "refresh-updates",
    },
    Tip {
        id: "show-burn-animals",
        summary: "Show an animated animal in the 5h percentage ring for how fast that window is being used.",
        detail: "The animal is the time until the window reaches 100% at the current pace: snail over 5h, turtle 3-5h, rabbit 1.5-3h, horse 30m-1.5h, cheetah under 30m. It appears in the 5h percentage ring only, taking turns with the percentage, once two refreshes in a row show usage rising, and only while that 5h usage changed in the last 10 minutes. Off by default; sorting by burn rate works either way.",
        example: "5h goes from 20% to 40% in 10 minutes: at that pace 100% is 30 minutes away, so the gauge shows a horse.",
        help_topic: "reading-a-card",
    },
    Tip {
        id: "auto-ping",
        summary: "Start idle usage windows automatically.",
        detail: "A window only starts counting at the first message. After each refresh, Auto Ping sends one short message from every active account whose 5h or 7d window is not running, so it resets sooner.",
        example: "An account idle since yesterday shows 5h with no reset time. Auto Ping starts it at 09:00, so it resets at 14:00 instead of five hours after you first use it.",
        help_topic: "ping",
    },
    Tip {
        id: "auto-switch",
        summary: "Move Claude Code to another account before a limit blocks you.",
        detail: "When the account Claude Code uses reaches a threshold, Claude Code moves to the account whose weekly quota is closest to going unused. Order: high critical, then the home account (bolt) after its 5h reset, then critical, then the threshold switch. Only accounts already signed in to Claude Code are used.",
        example: "Claude Code is on A at 5h 95%. B has 5h 10% and 7d 40% with two days left, so Claude Code moves to B.",
        help_topic: "auto-switch",
    },
    Tip {
        id: "five-hour-threshold",
        summary: "5h usage at which auto switch moves Claude Code.",
        detail: "When the active account's 5h usage reaches this, auto switch picks another account.",
        example: "At 95%, an account at 5h 94% stays; at 95% Claude Code moves.",
        help_topic: "auto-switch",
    },
    Tip {
        id: "seven-day-threshold",
        summary: "7d usage at which auto switch moves Claude Code.",
        detail: "When the active account's weekly usage reaches this, auto switch picks another account, whatever its 5h usage.",
        example: "At 99%, an account at 5h 20% but 7d 99% still moves Claude Code away.",
        help_topic: "auto-switch",
    },
    Tip {
        id: "menu-bar-toggle",
        summary: "Show the auto switch toggle in the tray flyout near a limit.",
        detail: "When auto switch is off, the flyout footer shows its toggle once the active account comes close to a threshold, so you can turn it on in time.",
        example: "Auto switch off, thresholds 95/99: at 5h 80% the toggle appears in the flyout footer.",
        help_topic: "auto-switch",
    },
    Tip {
        id: "critical-pace",
        summary: "Assumed use per 5h window, outside quiet hours, for the critical check.",
        detail: "An account is critical when, using this share of every 5h window from now until its weekly reset (quiet hours excluded), it still would not reach 100% of 7d. X (Dashboard header) converts 5h use into 7d use.",
        example: "X 16%, pace 50%: each window costs 8% of 7d. 7d at 60% with 30 active hours left (6 windows): 60 + 48 = 108, so not critical. 7d at 55% with 25 hours (5 windows): 55 + 40 = 95, so critical.",
        help_topic: "critical-accounts",
    },
    Tip {
        id: "high-critical-pace",
        summary: "Assumed use per 5h window, around the clock, for the high critical check.",
        detail: "An account is high critical when even this heavier pace, with no quiet hours, would not reach 100% of 7d by the reset. High critical accounts win over the home account.",
        example: "X 16%, pace 80%: 12.8% of 7d per window. 7d at 60% with 10 hours left (2 windows): 60 + 25.6 = 85.6, so high critical.",
        help_topic: "critical-accounts",
    },
    Tip {
        id: "quiet-hours",
        summary: "Daily hours not counted as usable for the critical check.",
        detail: "Hours between start and end each day are left out when counting the 5h windows still usable before a weekly reset. A start after the end crosses midnight. Equal start and end means no quiet hours. High critical ignores quiet hours.",
        example: "Quiet 00:00 to 07:00, reset in 48 hours from 10:00: two quiet blocks drop 14 hours, leaving 34 hours (6.8 windows).",
        help_topic: "critical-accounts",
    },
    Tip {
        id: "time-zone",
        summary: "Time zone the quiet hours are read in.",
        detail: "Quiet hours follow this zone, not the PC's. Useful when travelling or when your working day follows another zone.",
        example: "Zone Asia/Ho_Chi_Minh, quiet 00:00 to 07:00: on a PC set to Paris time that is 19:00 to 02:00 in summer.",
        help_topic: "critical-accounts",
    },
    Tip {
        id: "auto-update",
        summary: "Install new releases once a day.",
        detail: "Checks GitHub once a day and installs a newer version automatically. Check for Updates does the same on demand.",
        example: "A release published at noon is installed by the next daily check.",
        help_topic: "refresh-updates",
    },
];

/// The Slint model behind `TipStore.tips`.
pub fn model() -> slint::ModelRc<crate::TipData> {
    let items: Vec<crate::TipData> = TIPS
        .iter()
        .map(|t| crate::TipData {
            id: t.id.into(),
            summary: t.summary.into(),
            detail: t.detail.into(),
            example: t.example.into(),
            help_topic: t.help_topic.into(),
        })
        .collect();
    slint::ModelRc::new(slint::VecModel::from(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_ping_tip_matches_macos() {
        let t = TIPS.iter().find(|t| t.id == "auto-ping").expect("auto-ping tip");
        assert_eq!(t.summary, "Start idle usage windows automatically.");
        assert!(t.detail.starts_with("A window only starts counting at the first message."));
        assert!(t.example.contains("Auto Ping starts it at 09:00"));
        assert_eq!(t.help_topic, "ping");
    }

    #[test]
    fn show_burn_animals_tip_matches_macos() {
        let t = TIPS.iter().find(|t| t.id == "show-burn-animals").expect("show-burn-animals tip");
        assert_eq!(t.summary, "Show an animated animal in the 5h percentage ring for how fast that window is being used.");
        assert!(t.detail.contains("cheetah under 30m"));
        assert!(t.detail.contains("in the 5h percentage ring only"));
        assert!(t.detail.contains("changed in the last 10 minutes"));
        assert!(t.detail.contains("Off by default"));
        assert_eq!(t.help_topic, "reading-a-card");
    }

    #[test]
    fn every_tip_links_an_existing_help_topic() {
        assert!(!TIPS.is_empty());
        for t in TIPS {
            assert!(
                crate::help::TOPICS
                    .iter()
                    .any(|(id, _)| *id == t.help_topic),
                "tip {} links unknown help topic {}",
                t.id,
                t.help_topic
            );
            assert!(!t.summary.is_empty() && !t.detail.is_empty() && !t.example.is_empty());
        }
    }

    #[test]
    fn auto_switch_and_critical_tips_match_macos() {
        let tip = |id: &str| TIPS.iter().find(|t| t.id == id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(tip("auto-switch").summary, "Move Claude Code to another account before a limit blocks you.");
        assert_eq!(tip("auto-switch").help_topic, "auto-switch");
        assert_eq!(tip("five-hour-threshold").example, "At 95%, an account at 5h 94% stays; at 95% Claude Code moves.");
        assert_eq!(tip("seven-day-threshold").summary, "7d usage at which auto switch moves Claude Code.");
        assert_eq!(tip("menu-bar-toggle").summary, "Show the auto switch toggle in the tray flyout near a limit.");
        assert!(tip("critical-pace").example.starts_with("X 16%, pace 50%: each window costs 8% of 7d."));
        assert_eq!(tip("high-critical-pace").help_topic, "critical-accounts");
        assert!(tip("quiet-hours").detail.ends_with("High critical ignores quiet hours."));
        assert!(tip("time-zone").detail.starts_with("Quiet hours follow this zone, not the PC's."));
        assert!(tip("auto-refresh").detail.ends_with("(auto switch still polls the active account near a limit)."));
        assert!(tip("refresh-interval").example.starts_with("At 1 min, auto switch sees a 5h window reach 95% within a minute"));
    }

    #[test]
    fn tip_ids_are_unique_and_cover_the_settings() {
        assert_eq!(TIPS.len(), 12, "the 12 macOS SettingsTips");
        for id in [
            "auto-refresh", "refresh-interval", "auto-ping", "auto-switch", "five-hour-threshold", "seven-day-threshold",
            "menu-bar-toggle", "critical-pace", "high-critical-pace", "quiet-hours", "time-zone", "auto-update",
        ] {
            assert_eq!(TIPS.iter().filter(|t| t.id == id).count(), 1, "{id}");
        }
    }
}
