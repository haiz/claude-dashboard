//! The three kinds of account on Windows and the texts that depend on the kind
//! (Settings > Accounts label, expired-card hint). See the design spec,
//! "Account kinds on Windows".

use crate::extension_sources::ExtensionSources;
use crate::model::{Account, AccountSource, Browser};

#[derive(Debug, Clone, PartialEq)]
pub enum AccountKind {
    /// Read from a browser profile's cookie database.
    Scanned { browser: Browser, profile: String },
    /// Fed by the browser extension (a `manual` account with a binding).
    Extension { browser: Browser },
    /// A session key the user pasted by hand.
    PastedKey,
}

pub fn kind_of(a: &Account, sources: &ExtensionSources) -> AccountKind {
    if a.source == AccountSource::Browser {
        return AccountKind::Scanned {
            browser: a.browser.clone(),
            profile: a
                .chrome_profile_name
                .clone()
                .unwrap_or_else(|| a.chrome_profile_path.clone()),
        };
    }
    match sources.bindings.values().find(|b| b.account_id == a.id) {
        Some(b) => AccountKind::Extension {
            browser: Browser::from_extension_key(&b.browser).unwrap_or(Browser::Chrome),
        },
        None => AccountKind::PastedKey,
    }
}

impl AccountKind {
    /// The Settings > Accounts row label.
    pub fn settings_label(&self) -> String {
        match self {
            AccountKind::Scanned { profile, .. } => profile.clone(),
            AccountKind::Extension { browser } => {
                format!("Browser extension ({})", browser.display_name())
            }
            AccountKind::PastedKey => "Pasted key".to_string(),
        }
    }

    /// The hint under "Session expired." on the card.
    pub fn expired_hint(&self) -> String {
        match self {
            AccountKind::Scanned { browser, profile } => format!(
                "Open {} profile \"{}\" and login to claude.ai, then re-sync.",
                browser.display_name(),
                profile
            ),
            AccountKind::Extension { browser } => format!(
                "Open {} and sign in to claude.ai; the extension sends the new key.",
                browser.display_name()
            ),
            AccountKind::PastedKey => "This key was pasted by hand. Add it again from Settings \u{203A} Accounts, Add Account, \"Paste a session key instead\".".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountPlan, AccountStatus};

    fn acct(id: &str, source: AccountSource) -> Account {
        Account {
            id: id.into(),
            name: "n".into(),
            email: None,
            chrome_profile_path: "Profile 1".into(),
            chrome_profile_name: None,
            org_id: None,
            account_uuid: None,
            session_key: None,
            browser: Browser::Edge,
            plan: AccountPlan::Pro,
            last_synced: None,
            status: AccountStatus::Active,
            is_pinned: false,
            source,
        }
    }

    #[test]
    fn kind_of_scanned_extension_pasted() {
        let mut src = ExtensionSources::default();
        src.bind("inst", "ext", "edge");
        let mut s = acct("s", AccountSource::Browser);
        assert_eq!(
            kind_of(&s, &src),
            AccountKind::Scanned { browser: Browser::Edge, profile: "Profile 1".into() }
        );
        s.chrome_profile_name = Some("Work".into());
        assert_eq!(
            kind_of(&s, &src),
            AccountKind::Scanned { browser: Browser::Edge, profile: "Work".into() }
        );
        assert_eq!(
            kind_of(&acct("ext", AccountSource::Manual), &src),
            AccountKind::Extension { browser: Browser::Edge }
        );
        assert_eq!(kind_of(&acct("p", AccountSource::Manual), &src), AccountKind::PastedKey);
        // Unknown binding browser falls back to Chrome.
        src.bind("i2", "weird", "opera");
        assert_eq!(
            kind_of(&acct("weird", AccountSource::Manual), &src),
            AccountKind::Extension { browser: Browser::Chrome }
        );
    }

    #[test]
    fn labels_and_hints_name_the_real_browser() {
        let ext = AccountKind::Extension { browser: Browser::Edge };
        assert_eq!(ext.settings_label(), "Browser extension (Microsoft Edge)");
        assert_eq!(
            ext.expired_hint(),
            "Open Microsoft Edge and sign in to claude.ai; the extension sends the new key."
        );
        let sc = AccountKind::Scanned { browser: Browser::Brave, profile: "Work".into() };
        assert_eq!(sc.settings_label(), "Work");
        assert_eq!(
            sc.expired_hint(),
            "Open Brave profile \"Work\" and login to claude.ai, then re-sync."
        );
        assert_eq!(AccountKind::PastedKey.settings_label(), "Pasted key");
        assert!(AccountKind::PastedKey.expired_hint().starts_with("This key was pasted by hand."));
    }
}
