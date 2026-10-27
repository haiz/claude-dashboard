//! The Claude Desktop part of the Switch flow (macOS `DashboardViewModel` Claude Desktop
//! Switch section): what follows a Claude Code switch, and the Desktop texts.

use claude_dashboard_core::auto_switch_log::{NewEntry, Outcome, SwitchTarget, Trigger};
use claude_dashboard_core::cd_switch::switcher::{DesktopAvailability, DesktopSwitchError};

pub const CLOSE_TITLE: &str = "Close Claude Desktop?";
/// macOS `desktopCloseMessage`.
pub const CLOSE_MESSAGE: &str =
    "Claude Desktop must be closed to switch it. Close it now and switch? Unsaved work in Desktop may be lost.";
pub const CLOSE_BUTTON: &str = "Close and Switch";
/// Shown by `CcPrompt` in app.slint; kept here with the other macOS texts.
#[allow(dead_code)]
pub const CHECKBOX: &str = "Switch Claude Desktop too";

/// What follows a Claude Code switch (macOS `switchClaudeCodeThenDesktop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopStep {
    None,
    /// Desktop is running and the box was ticked: ask before closing it.
    AskClose,
    /// Desktop was closed and the setting is on: switch it straight away (the switcher
    /// refuses if it launched meanwhile).
    RunNow,
}

pub fn after_cc_switch(cc_switched: bool, ticked: bool, enabled: bool, was_running: bool) -> DesktopStep {
    if !cc_switched {
        DesktopStep::None
    } else if ticked {
        DesktopStep::AskClose
    } else if enabled && !was_running {
        DesktopStep::RunNow
    } else {
        DesktopStep::None
    }
}

/// The box starts ticked when the setting is on and Desktop runs (it shows only then).
pub fn initial_tick(enabled: bool, running: bool) -> bool {
    enabled && running
}

pub fn success_message(name: &str) -> String {
    format!("Claude Desktop now uses {name}. Open it to continue.")
}

pub fn did_not_close_message(name: &str) -> String {
    format!("Claude Desktop did not close, so it was not switched to {name}.")
}

/// macOS `desktopMessage`; `has_profile` is whether a browser capture was possible.
pub fn error_message(e: &DesktopSwitchError, name: &str, has_profile: bool) -> String {
    match e {
        DesktopSwitchError::DesktopRunning => {
            format!("Claude Desktop is running, so it was not switched to {name}. Close it and try again.")
        }
        DesktopSwitchError::NotCaptured | DesktopSwitchError::LoginExpired => {
            if has_profile {
                format!("Could not sign Claude Desktop in as {name}. Try the Desktop switch again.")
            } else {
                format!("Re-sync {name} first so the dashboard knows its browser profile.")
            }
        }
        DesktopSwitchError::NoSessionKey => {
            format!("Re-sync {name} first so the dashboard has its claude.ai session for Claude Desktop.")
        }
        DesktopSwitchError::UnknownOutgoingAccount { email } => format!(
            "Claude Desktop is signed in as {}, whose login cannot be saved, so it was left alone.",
            email.as_deref().unwrap_or("an account the dashboard does not know")
        ),
        DesktopSwitchError::Config | DesktopSwitchError::VerifyFailed => {
            format!("Could not write Claude Desktop's login for {name}. Claude Desktop was left as it was.")
        }
        DesktopSwitchError::Provision(m) => m.clone(),
    }
}

/// The Desktop outcome under the Claude Code one, on its own line (macOS `noteDesktop`).
pub fn join(cc: Option<&str>, desktop: &str) -> String {
    match cc {
        Some(c) => format!("{c}\n{desktop}"),
        None => desktop.to_string(),
    }
}

/// A manual Desktop switch's Switch Log row (macOS `logDesktopSwitch`): the detail is the
/// error's case name on a failure, none on success.
pub fn log_entry(name: &str, error: Option<&DesktopSwitchError>, now: f64) -> NewEntry {
    NewEntry {
        date_unix: now,
        trigger: Trigger::Manual,
        target: SwitchTarget::ClaudeDesktop,
        outcome: if error.is_some() { Outcome::Failed } else { Outcome::Switched },
        from_name: None,
        to_name: Some(name.to_string()),
        reason: "Manual switch".into(),
        detail: error.map(|e| e.log_name().to_string()),
    }
}

/// What Auto Switch does for Claude Desktop after moving Claude Code (macOS `desktopLookAhead`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookAhead {
    /// Setting off, or Desktop runs: leave it alone and log nothing.
    Off,
    /// Desktop has no usable captured login for the target: log a skip.
    Skip,
    /// Write the target's login into the closed Desktop.
    Run,
}

pub fn look_ahead(enabled: bool, running: bool, availability: Option<DesktopAvailability>) -> LookAhead {
    if !enabled || running {
        LookAhead::Off
    } else if availability == Some(DesktopAvailability::Ready) {
        LookAhead::Run
    } else {
        LookAhead::Skip
    }
}

/// An Auto Switch look-ahead's Switch Log row: `error` is the failure of a `Run`, `skipped`
/// marks a `Skip` (no Desktop capture); otherwise it switched.
pub fn look_ahead_entry(name: &str, skipped: bool, error: Option<&DesktopSwitchError>, now: f64) -> NewEntry {
    let (outcome, detail) = if skipped {
        (Outcome::Skipped, Some("Not captured for Desktop".to_string()))
    } else if let Some(e) = error {
        (Outcome::Failed, Some(e.log_name().to_string()))
    } else {
        (Outcome::Switched, None)
    };
    NewEntry {
        date_unix: now,
        trigger: Trigger::Auto,
        target: SwitchTarget::ClaudeDesktop,
        outcome,
        from_name: None,
        to_name: Some(name.to_string()),
        reason: "Desktop look-ahead".into(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_cc_switch_table() {
        use DesktopStep::*;
        // (cc_switched, ticked, enabled, was_running) -> step
        let table = [
            ((false, false, false, false), None),
            ((false, false, false, true), None),
            ((false, false, true, false), None),
            ((false, false, true, true), None),
            ((false, true, false, false), None),
            ((false, true, false, true), None),
            ((false, true, true, false), None),
            ((false, true, true, true), None),
            ((true, false, false, false), None),
            ((true, false, false, true), None),
            ((true, false, true, false), RunNow),
            ((true, false, true, true), None),
            ((true, true, false, false), AskClose),
            ((true, true, false, true), AskClose),
            ((true, true, true, false), AskClose),
            ((true, true, true, true), AskClose),
        ];
        for ((switched, ticked, enabled, running), expected) in table {
            assert_eq!(
                after_cc_switch(switched, ticked, enabled, running),
                expected,
                "{switched} {ticked} {enabled} {running}"
            );
        }
    }

    #[test]
    fn cancel_path_none_when_cc_failed() {
        for (ticked, enabled, running) in [(true, true, true), (false, true, false), (true, false, false)] {
            assert_eq!(after_cc_switch(false, ticked, enabled, running), DesktopStep::None);
        }
    }

    #[test]
    fn initial_tick_table() {
        assert!(initial_tick(true, true));
        assert!(!initial_tick(true, false));
        assert!(!initial_tick(false, true));
        assert!(!initial_tick(false, false));
    }

    #[test]
    fn texts_match_macos() {
        assert_eq!(CLOSE_TITLE, "Close Claude Desktop?");
        assert_eq!(
            CLOSE_MESSAGE,
            "Claude Desktop must be closed to switch it. Close it now and switch? Unsaved work in Desktop may be lost."
        );
        assert_eq!(CLOSE_BUTTON, "Close and Switch");
        assert_eq!(CHECKBOX, "Switch Claude Desktop too");
        assert_eq!(success_message("b@x.com"), "Claude Desktop now uses b@x.com. Open it to continue.");
        assert_eq!(
            did_not_close_message("b@x.com"),
            "Claude Desktop did not close, so it was not switched to b@x.com."
        );
    }

    #[test]
    fn error_message_per_variant() {
        let n = "b@x.com";
        assert_eq!(
            error_message(&DesktopSwitchError::DesktopRunning, n, true),
            "Claude Desktop is running, so it was not switched to b@x.com. Close it and try again."
        );
        for e in [DesktopSwitchError::NotCaptured, DesktopSwitchError::LoginExpired] {
            assert_eq!(
                error_message(&e, n, true),
                "Could not sign Claude Desktop in as b@x.com. Try the Desktop switch again."
            );
            assert_eq!(error_message(&e, n, false), "Re-sync b@x.com first so the dashboard knows its browser profile.");
        }
        assert_eq!(
            error_message(&DesktopSwitchError::NoSessionKey, n, true),
            "Re-sync b@x.com first so the dashboard has its claude.ai session for Claude Desktop."
        );
        assert_eq!(
            error_message(&DesktopSwitchError::UnknownOutgoingAccount { email: Some("a@x.com".into()) }, n, true),
            "Claude Desktop is signed in as a@x.com, whose login cannot be saved, so it was left alone."
        );
        assert_eq!(
            error_message(&DesktopSwitchError::UnknownOutgoingAccount { email: None }, n, false),
            "Claude Desktop is signed in as an account the dashboard does not know, whose login cannot be saved, so it was left alone."
        );
        for e in [DesktopSwitchError::Config, DesktopSwitchError::VerifyFailed] {
            for p in [true, false] {
                assert_eq!(
                    error_message(&e, n, p),
                    "Could not write Claude Desktop's login for b@x.com. Claude Desktop was left as it was."
                );
            }
        }
        assert_eq!(error_message(&DesktopSwitchError::Provision("Sign-in timed out.".into()), n, true), "Sign-in timed out.");
    }

    #[test]
    fn log_entry_for_a_manual_desktop_switch() {
        let ok = log_entry("b@x.com", None, 12.0);
        assert_eq!(ok.date_unix, 12.0);
        assert_eq!(ok.trigger, Trigger::Manual);
        assert_eq!(ok.target, SwitchTarget::ClaudeDesktop);
        assert_eq!(ok.outcome, Outcome::Switched);
        assert_eq!(ok.from_name, None);
        assert_eq!(ok.to_name.as_deref(), Some("b@x.com"));
        assert_eq!(ok.reason, "Manual switch");
        assert_eq!(ok.detail, None);
        let failed = log_entry("b@x.com", Some(&DesktopSwitchError::VerifyFailed), 12.0);
        assert_eq!(failed.outcome, Outcome::Failed);
        assert_eq!(failed.detail.as_deref(), Some("verifyFailed"));
    }

    #[test]
    fn look_ahead_table() {
        use DesktopAvailability::*;
        assert_eq!(look_ahead(true, false, Some(Ready)), LookAhead::Run); // writes_when_closed
        assert_eq!(look_ahead(true, true, Some(Ready)), LookAhead::Off); // skipped_when_running
        assert_eq!(look_ahead(false, false, Some(Ready)), LookAhead::Off); // off_when_disabled
        for a in [Some(NotCaptured), Some(LoginExpired), Some(Active), None] {
            assert_eq!(look_ahead(true, false, a), LookAhead::Skip, "{a:?}"); // skips_uncaptured
            assert_eq!(look_ahead(true, true, a), LookAhead::Off);
            assert_eq!(look_ahead(false, false, a), LookAhead::Off);
        }
    }

    #[test]
    fn look_ahead_rows() {
        let ok = look_ahead_entry("b@x.com", false, None, 5.0);
        assert_eq!((ok.trigger, ok.target, ok.outcome), (Trigger::Auto, SwitchTarget::ClaudeDesktop, Outcome::Switched));
        assert_eq!((ok.reason.as_str(), ok.detail.as_deref(), ok.from_name.as_deref()), ("Desktop look-ahead", None, None));
        let failed = look_ahead_entry("b@x.com", false, Some(&DesktopSwitchError::DesktopRunning), 5.0);
        assert_eq!(failed.outcome, Outcome::Failed);
        assert_eq!(failed.detail.as_deref(), Some("desktopRunning"));
        let skipped = look_ahead_entry("b@x.com", true, None, 5.0);
        assert_eq!(skipped.outcome, Outcome::Skipped);
        assert_eq!(skipped.detail.as_deref(), Some("Not captured for Desktop"));
        assert_eq!(skipped.to_name.as_deref(), Some("b@x.com"));
    }

    #[test]
    fn join_two_lines() {
        assert_eq!(join(Some("cc"), "desktop"), "cc\ndesktop");
    }

    #[test]
    fn join_without_cc_message() {
        assert_eq!(join(None, "desktop"), "desktop");
    }
}
