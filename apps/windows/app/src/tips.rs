//! InfoTip texts for Settings > General, copied from macOS `SettingsTips.swift`
//! (Auto Refresh, Auto Ping, Updates; Auto Switch and Critical tips come with their projects).

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
        detail: "When on, the app fetches usage for all accounts every interval. When off, usage only updates when you press Refresh.",
        example: "With a 5 min interval, a card that shows 40% at 10:00 shows the new figure by 10:05.",
        help_topic: "refresh-updates",
    },
    Tip {
        id: "refresh-interval",
        summary: "Minutes between automatic refreshes (1 to 60).",
        detail: "Shorter intervals keep cards fresher, at the cost of more requests to claude.ai.",
        example: "At 1 min a card is at most a minute out of date; at 15 min it can be 15 minutes behind.",
        help_topic: "refresh-updates",
    },
    Tip {
        id: "auto-ping",
        summary: "Start idle usage windows automatically.",
        detail: "A window only starts counting at the first message. After each refresh, Auto Ping sends one short message from every active account whose 5h or 7d window is not running, so it resets sooner.",
        example: "An account idle since yesterday shows 5h with no reset time. Auto Ping starts it at 09:00, so it resets at 14:00 instead of five hours after you first use it.",
        help_topic: "ping",
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
    fn tip_ids_are_unique_and_cover_the_settings() {
        for id in ["auto-refresh", "refresh-interval", "auto-update"] {
            assert_eq!(TIPS.iter().filter(|t| t.id == id).count(), 1, "{id}");
        }
    }
}
