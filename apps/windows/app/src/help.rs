//! Help pane topics: ids and titles in macOS order (`HelpTopic.allCases` minus the
//! topics Windows does not have yet). The cards themselves live in `ui/help.slint`;
//! the ids here are the contract for `help-focus` and `open-help`.

/// `(id, title)` per topic, in display order.
pub const TOPICS: &[(&str, &str)] = &[
    ("getting-started", "Getting Started"),
    ("reading-a-card", "Reading a Card"),
    ("finding-your-way", "Finding Your Way"),
    ("charts", "Charts"),
    ("switch-claude-code", "Switch Claude Code"),
    ("switch-claude-desktop", "Switch Claude Desktop"),
    ("auto-switch", "Auto Switch"),
    ("critical-accounts", "Critical Accounts"),
    ("ping", "Ping"),
    ("managing-accounts", "Managing Accounts"),
    ("refresh-updates", "Refresh & Updates"),
    ("troubleshooting", "Troubleshooting"),
];

/// The topic model for the Help pane's grid.
pub fn topic_model() -> slint::ModelRc<crate::HelpTopic> {
    let items: Vec<crate::HelpTopic> = TOPICS
        .iter()
        .map(|(id, title)| crate::HelpTopic {
            id: (*id).into(),
            title: (*title).into(),
        })
        .collect();
    slint::ModelRc::new(slint::VecModel::from(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_in_macos_order() {
        assert_eq!(
            TOPICS,
            &[
                ("getting-started", "Getting Started"),
                ("reading-a-card", "Reading a Card"),
                ("finding-your-way", "Finding Your Way"),
                ("charts", "Charts"),
                ("switch-claude-code", "Switch Claude Code"),
                ("switch-claude-desktop", "Switch Claude Desktop"),
                ("auto-switch", "Auto Switch"),
                ("critical-accounts", "Critical Accounts"),
                ("ping", "Ping"),
                ("managing-accounts", "Managing Accounts"),
                ("refresh-updates", "Refresh & Updates"),
                ("troubleshooting", "Troubleshooting"),
            ]
        );
    }

    #[test]
    fn finding_your_way_names_ping_log_and_auto_ping() {
        let ui = include_str!("../ui/help.slint");
        assert!(ui.contains("**Tools**: Ping Log, Switch Log and Help."));
        assert!(ui.contains("**Settings**: accounts, refresh, Auto Ping, Claude Desktop, Auto Switch, updates."));
        assert!(ui.contains("The **Auto switch** toggle sits bottom right with its latest event."));
    }

    #[test]
    fn has_switch_claude_desktop_topic() {
        let ui = include_str!("../ui/help.slint");
        assert!(TOPICS.contains(&("switch-claude-desktop", "Switch Claude Desktop")));
        for text in [
            "subtitle: \"Move Claude Desktop along with Claude Code.\"",
            "Turn on **Switch Claude Desktop with Switch** in **Settings › General › Claude Desktop** (off by default).",
            "**Close and Switch** quits it (unsaved work in Desktop may be lost)",
            "**Desktop closed**: it is switched straight away, with no question.",
            "Claude Desktop gets **its own login**",
            "**Switch Desktop to this account**: on the page of the account Claude Code uses",
            "Recommended: Claude Code and Claude Desktop",
            "Recommended: Claude Desktop only",
            "the tray icon then shows its 5h usage",
            "**Claude Desktop**: moved only with **Let auto switch move Desktop when it is closed** on",
            "**Only use Claude Desktop?** Turn Auto Switch off.",
            "Claude Desktop rows carry a **Desktop** tag.",
            "Claude Desktop was not switched",
        ] {
            assert!(ui.contains(text), "{text}");
        }
    }

    #[test]
    fn auto_switch_and_critical_cards_match_macos() {
        let ui = include_str!("../ui/help.slint");
        for text in [
            "Auto Switch is **on by default**.",
            "**First, critical accounts**",
            "**Near a 5h reset**",
            "**On failure**: that account is skipped for 10 minutes.",
            "An account is **critical** when its weekly (7d) quota will expire unused",
            "Formula: 7d now + (usable hours left / 5) x pace x X.",
            "**3.** 7d at 60%, reset in 10 hours.",
            "**Home after reset**",
            "**Flame**: orange when the account is critical, red when high critical",
            "**X** (Dashboard header)",
            "**Switch after reset**",
            "Auto Switch did nothing",
        ] {
            assert!(ui.contains(text), "{text}");
        }
    }

    #[test]
    fn every_topic_has_a_card_in_the_slint_file() {
        let ui = include_str!("../ui/help.slint");
        for (id, title) in TOPICS {
            assert!(
                ui.contains(&format!("topic-id: \"{id}\"")),
                "no card for {id}"
            );
            assert!(
                ui.contains(&format!("title: \"{title}\"")),
                "no title for {id}"
            );
        }
    }
}
