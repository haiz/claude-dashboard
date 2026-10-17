//! Help pane topics: ids and titles in macOS order (`HelpTopic.allCases` minus the
//! topics Windows does not have yet). The cards themselves live in `ui/help.slint`;
//! the ids here are the contract for `help-focus` and `open-help`.

/// `(id, title)` per topic, in display order.
pub const TOPICS: &[(&str, &str)] = &[
    ("getting-started", "Getting Started"),
    ("reading-a-card", "Reading a Card"),
    ("finding-your-way", "Finding Your Way"),
    ("charts", "Charts"),
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
                ("managing-accounts", "Managing Accounts"),
                ("refresh-updates", "Refresh & Updates"),
                ("troubleshooting", "Troubleshooting"),
            ]
        );
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
