//! Per-kind re-sync: a port of macOS `DashboardViewModel.resyncCore`, plus the
//! Windows account kinds (see `claude_dashboard_core::account_kind`).
//!
//! * Scanned: re-read the key from the stored browser profile, stop if the
//!   profile now holds a different Claude account, otherwise save the key and
//!   re-resolve the org.
//! * Extension: nothing to read; the caller nudges a refresh.
//! * Pasted key: nothing to read; the user pastes a new one.
//!
//! All I/O that is not the account store goes through [`ResyncEnv`], so tests
//! touch no browser and no network.

use claude_dashboard_core::account_kind::{kind_of, AccountKind};
use claude_dashboard_core::api::{self, ParsedAccount};
use claude_dashboard_core::extension_sources;
use claude_dashboard_core::identity::resolve_org_id;
use claude_dashboard_core::model::{AccountStatus, Browser};
use claude_dashboard_core::plan::{parse_orgs, refreshed_plan_for};
use claude_dashboard_core::scan::{read_profile_session_key, ResyncReadError};
use claude_dashboard_core::store;

#[derive(Debug, Clone, PartialEq)]
pub enum ResyncOutcome {
    /// A scanned account got a fresh key from its profile.
    Updated,
    /// An extension-fed account: nothing was read; the caller refreshes usage.
    Refreshed,
    Failed(String),
}

pub trait ResyncEnv {
    /// The profile's sessionKey and its `lastActiveOrg` cookie, if any.
    fn read_key(
        &self,
        b: Browser,
        profile: &str,
    ) -> Result<(String, Option<String>), ResyncReadError>;
    fn fetch_account(&self, key: &str) -> Result<ParsedAccount, String>;
    /// The raw `/api/organizations` body.
    fn fetch_orgs(&self, key: &str) -> Result<String, String>;
}

pub struct RealResyncEnv;

impl ResyncEnv for RealResyncEnv {
    fn read_key(
        &self,
        b: Browser,
        profile: &str,
    ) -> Result<(String, Option<String>), ResyncReadError> {
        read_profile_session_key(b, profile)
    }
    fn fetch_account(&self, key: &str) -> Result<ParsedAccount, String> {
        let body = api::fetch_account(key).map_err(|e| e.to_string())?;
        api::parse_account(&body).ok_or_else(|| "could not identify this session".to_string())
    }
    fn fetch_orgs(&self, key: &str) -> Result<String, String> {
        api::fetch_organizations(key).map_err(|e| e.to_string())
    }
}

fn failed(msg: &str) -> ResyncOutcome {
    ResyncOutcome::Failed(msg.to_string())
}

/// Re-syncs one account according to its kind.
pub fn resync(account_id: &str, env: &dyn ResyncEnv) -> ResyncOutcome {
    let accounts = match store::load_accounts() {
        Ok(a) => a,
        Err(e) => return ResyncOutcome::Failed(format!("Re-sync failed. {e}")),
    };
    let Some(account) = accounts.iter().find(|a| a.id == account_id) else {
        return failed("Re-sync failed. This account no longer exists.");
    };
    let sources = extension_sources::load(&extension_sources::sources_path()).unwrap_or_default();

    let (browser, profile) = match kind_of(account, &sources) {
        AccountKind::Extension { .. } => return ResyncOutcome::Refreshed,
        AccountKind::PastedKey => {
            return failed(
                "This account's session key was pasted by hand. Paste a new one to re-sync it.",
            )
        }
        AccountKind::Scanned { browser, profile } => (browser, profile),
    };

    let (key, cookie_org) =
        match env.read_key(account.browser.clone(), &account.chrome_profile_path) {
            Ok(k) => k,
            Err(ResyncReadError::AppBound) => {
                return ResyncOutcome::Failed(format!(
                    "Re-sync failed. {} protects this profile's cookies (app-bound encryption). \
                 Use the browser extension for this account instead.",
                    browser.display_name()
                ))
            }
            Err(ResyncReadError::NoSession) => {
                return ResyncOutcome::Failed(format!(
                    "Re-sync failed. Open {} profile \"{}\" and login to claude.ai first.",
                    browser.display_name(),
                    profile
                ))
            }
        };

    // Who the profile is signed in as now. A failure is not fatal: the new key
    // is still worth saving, and `Active` is what lets the next refresh retry.
    let info = env.fetch_account(&key).ok();
    if let (Some(info), Some(stored)) = (&info, &account.account_uuid) {
        if &info.uuid != stored {
            return failed(
                "Re-sync stopped. That browser profile is now signed in to a different Claude account.",
            );
        }
    }
    // Plan hint only; a failed fetch just leaves the plan as it is.
    let orgs = env
        .fetch_orgs(&key)
        .map(|b| parse_orgs(&b))
        .unwrap_or_default();

    let _lock = match store::lock_store() {
        Ok(l) => l,
        Err(e) => return ResyncOutcome::Failed(format!("Re-sync failed. {e}")),
    };
    // Reload under the lock: another writer may have changed the list while we
    // were on the network.
    let (mut accounts, _) = match store::load_accounts_for_write() {
        Ok(v) => v,
        Err(e) => return ResyncOutcome::Failed(format!("Re-sync failed. {e}")),
    };
    let Some(target) = accounts.iter_mut().find(|a| a.id == account_id) else {
        return failed("Re-sync failed. This account no longer exists.");
    };

    target.session_key = Some(store::encrypt_session_key(&key));
    target.status = AccountStatus::Active;

    // The org follows the org-selection rule, never a raw cookie. The stored org
    // is the fallback preference when the profile has no `lastActiveOrg` cookie.
    // When /api/account was unreachable the stored org is untouched.
    let mut org_unresolved = false;
    if let Some(info) = &info {
        if target.account_uuid.is_none() {
            target.account_uuid = Some(info.uuid.clone());
        }
        if target.email.is_none() {
            target.email = info.email.clone();
        }
        match resolve_org_id(
            cookie_org
                .as_deref()
                .filter(|s| !s.is_empty())
                .or(target.org_id.as_deref()),
            &info.memberships,
        ) {
            Some(org) => target.org_id = Some(org),
            None => org_unresolved = true,
        }
    }
    if let Some(plan) = refreshed_plan_for(target, &orgs) {
        target.plan = plan;
    }
    if let Err(e) = store::save_accounts(&accounts) {
        return ResyncOutcome::Failed(format!("Re-sync failed. {e}"));
    }
    if org_unresolved {
        return failed("Re-sync incomplete. This account has no organization with chat access.");
    }
    ResyncOutcome::Updated
}

fn scanned_in(
    accounts: &[claude_dashboard_core::model::Account],
    sources: &extension_sources::ExtensionSources,
    id: &str,
) -> bool {
    accounts
        .iter()
        .find(|a| a.id == id)
        .is_some_and(|a| matches!(kind_of(a, sources), AccountKind::Scanned { .. }))
}

/// How many of `ids` are scanned accounts: the denominator of Re-sync All's progress.
pub fn scanned_total(ids: &[String]) -> usize {
    let accounts = store::load_accounts().unwrap_or_default();
    let sources = extension_sources::load(&extension_sources::sources_path()).unwrap_or_default();
    ids.iter()
        .filter(|id| scanned_in(&accounts, &sources, id))
        .count()
}

/// Re-syncs every id in order. `progress(done, total)` is called after each
/// *scanned* account finishes; `total` counts scanned accounts only.
pub fn resync_all(
    ids: &[String],
    env: &dyn ResyncEnv,
    progress: &dyn Fn(usize, usize),
) -> Vec<(String, ResyncOutcome)> {
    let accounts = store::load_accounts().unwrap_or_default();
    let sources = extension_sources::load(&extension_sources::sources_path()).unwrap_or_default();
    let is_scanned = |id: &str| scanned_in(&accounts, &sources, id);
    let total = ids.iter().filter(|id| is_scanned(id)).count();
    let mut done = 0;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let outcome = resync(id, env);
        if is_scanned(id) {
            done += 1;
            progress(done, total);
        }
        out.push((id.clone(), outcome));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::identity::OrgMembership;
    use claude_dashboard_core::model::{Account, AccountPlan, AccountSource};
    use std::cell::RefCell;

    struct Fake {
        key: Result<String, ResyncReadError>,
        cookie_org: Option<String>,
        account: Result<ParsedAccount, String>,
        read_calls: RefCell<Vec<(String, String)>>,
    }

    impl ResyncEnv for Fake {
        fn read_key(
            &self,
            b: Browser,
            profile: &str,
        ) -> Result<(String, Option<String>), ResyncReadError> {
            self.read_calls
                .borrow_mut()
                .push((b.display_name().to_string(), profile.to_string()));
            self.key.clone().map(|k| (k, self.cookie_org.clone()))
        }
        fn fetch_account(&self, _key: &str) -> Result<ParsedAccount, String> {
            self.account.clone()
        }
        fn fetch_orgs(&self, _key: &str) -> Result<String, String> {
            Err("offline".into())
        }
    }

    fn fake(key: Result<&str, ResyncReadError>, uuid: &str) -> Fake {
        Fake {
            key: key.map(String::from),
            cookie_org: None,
            account: Ok(ParsedAccount {
                uuid: uuid.into(),
                email: Some("a@b.c".into()),
                memberships: ["org-new", "org-cookie", "org-stored"]
                    .iter()
                    .map(|u| OrgMembership {
                        uuid: u.to_string(),
                        name: "Org".into(),
                        capabilities: vec!["chat".into()],
                    })
                    .collect(),
            }),
            read_calls: RefCell::new(Vec::new()),
        }
    }

    fn account(id: &str, source: AccountSource) -> Account {
        Account {
            id: id.into(),
            name: id.into(),
            email: None,
            chrome_profile_path: if source == AccountSource::Browser {
                "Profile 2".into()
            } else {
                String::new()
            },
            chrome_profile_name: None,
            org_id: Some("org-old".into()),
            account_uuid: Some("uuid-1".into()),
            session_key: Some(store::encrypt_session_key("sk-old")),
            browser: Browser::Edge,
            plan: AccountPlan::Pro,
            last_synced: None,
            status: AccountStatus::Expired,
            is_pinned: false,
            source,
        }
    }

    /// Points APPDATA at a tempdir and seeds the store.
    fn setup(accounts: &[Account], bind: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());
        store::save_accounts(accounts).unwrap();
        if !bind.is_empty() {
            let mut s = extension_sources::ExtensionSources::default();
            for (i, (acct, browser)) in bind.iter().enumerate() {
                s.bind(&format!("inst{i}"), acct, browser);
            }
            extension_sources::save(&extension_sources::sources_path(), &s).unwrap();
        }
        dir
    }

    fn stored(id: &str) -> Account {
        store::load_accounts()
            .unwrap()
            .into_iter()
            .find(|a| a.id == id)
            .unwrap()
    }

    #[test]
    fn scanned_same_account_updates_key_and_org() {
        let _g = crate::testenv::lock();
        let _d = setup(&[account("s", AccountSource::Browser)], &[]);
        let env = fake(Ok("sk-new"), "uuid-1");
        assert_eq!(resync("s", &env), ResyncOutcome::Updated);
        let a = stored("s");
        assert_eq!(
            store::decrypt_session_key(a.session_key.as_deref().unwrap()).as_deref(),
            Some("sk-new")
        );
        assert_eq!(a.org_id.as_deref(), Some("org-new"));
        assert_eq!(a.status, AccountStatus::Active);
        assert_eq!(
            env.read_calls.borrow()[0],
            ("Microsoft Edge".to_string(), "Profile 2".to_string())
        );
    }

    #[test]
    fn cookie_org_wins_over_stored_org_and_stored_is_the_fallback() {
        let _g = crate::testenv::lock();
        let mut a = account("s", AccountSource::Browser);
        a.org_id = Some("org-stored".into());
        let _d = setup(&[a], &[]);

        let mut env = fake(Ok("sk-new"), "uuid-1");
        env.cookie_org = Some("org-cookie".into());
        assert_eq!(resync("s", &env), ResyncOutcome::Updated);
        assert_eq!(stored("s").org_id.as_deref(), Some("org-cookie"));

        // No cookie org: the stored org is the preference.
        let mut a = stored("s");
        a.org_id = Some("org-stored".into());
        store::save_accounts(&[a]).unwrap();
        let env = fake(Ok("sk-new"), "uuid-1");
        assert_eq!(resync("s", &env), ResyncOutcome::Updated);
        assert_eq!(stored("s").org_id.as_deref(), Some("org-stored"));
    }

    #[test]
    fn empty_cookie_org_falls_back_to_stored_org() {
        let _g = crate::testenv::lock();
        let mut a = account("s", AccountSource::Browser);
        a.org_id = Some("org-stored".into());
        let _d = setup(&[a], &[]);
        let mut env = fake(Ok("sk-new"), "uuid-1");
        env.cookie_org = Some(String::new());
        assert_eq!(resync("s", &env), ResyncOutcome::Updated);
        assert_eq!(stored("s").org_id.as_deref(), Some("org-stored"));
    }

    #[test]
    fn scanned_different_account_writes_nothing() {
        let _g = crate::testenv::lock();
        let _d = setup(&[account("s", AccountSource::Browser)], &[]);
        let env = fake(Ok("sk-new"), "uuid-OTHER");
        assert_eq!(
            resync("s", &env),
            ResyncOutcome::Failed(
                "Re-sync stopped. That browser profile is now signed in to a different Claude account.".into()
            )
        );
        let a = stored("s");
        assert_eq!(
            store::decrypt_session_key(a.session_key.as_deref().unwrap()).as_deref(),
            Some("sk-old")
        );
        assert_eq!(a.status, AccountStatus::Expired);
        assert_eq!(a.org_id.as_deref(), Some("org-old"));
    }

    #[test]
    fn scanned_no_cookie_fails_with_browser_name() {
        let _g = crate::testenv::lock();
        let mut a = account("s", AccountSource::Browser);
        a.chrome_profile_name = Some("Work".into());
        let _d = setup(&[a], &[]);
        let env = fake(Err(ResyncReadError::NoSession), "uuid-1");
        assert_eq!(
            resync("s", &env),
            ResyncOutcome::Failed(
                "Re-sync failed. Open Microsoft Edge profile \"Work\" and login to claude.ai first.".into()
            )
        );
        assert_eq!(stored("s").status, AccountStatus::Expired);
    }

    #[test]
    fn extension_refreshes_only() {
        let _g = crate::testenv::lock();
        let _d = setup(&[account("e", AccountSource::Manual)], &[("e", "edge")]);
        let env = fake(Ok("sk-new"), "uuid-1");
        assert_eq!(resync("e", &env), ResyncOutcome::Refreshed);
        assert!(env.read_calls.borrow().is_empty());
        let a = stored("e");
        assert_eq!(
            store::decrypt_session_key(a.session_key.as_deref().unwrap()).as_deref(),
            Some("sk-old")
        );
        assert_eq!(a.status, AccountStatus::Expired);
    }

    #[test]
    fn pasted_fails_with_paste_message() {
        let _g = crate::testenv::lock();
        let _d = setup(&[account("p", AccountSource::Manual)], &[]);
        let env = fake(Ok("sk-new"), "uuid-1");
        assert_eq!(
            resync("p", &env),
            ResyncOutcome::Failed(
                "This account's session key was pasted by hand. Paste a new one to re-sync it."
                    .into()
            )
        );
        assert!(env.read_calls.borrow().is_empty());
    }

    #[test]
    fn resync_all_progress_counts_scanned_only() {
        let _g = crate::testenv::lock();
        let _d = setup(
            &[
                account("s", AccountSource::Browser),
                account("e", AccountSource::Manual),
                account("p", AccountSource::Manual),
            ],
            &[("e", "chrome")],
        );
        let env = fake(Ok("sk-new"), "uuid-1");
        let calls = RefCell::new(Vec::new());
        let ids: Vec<String> = ["s", "e", "p"].iter().map(|s| s.to_string()).collect();
        let out = resync_all(&ids, &env, &|d, t| calls.borrow_mut().push((d, t)));
        assert_eq!(*calls.borrow(), vec![(1, 1)]);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].1, ResyncOutcome::Updated);
        assert_eq!(out[1].1, ResyncOutcome::Refreshed);
        assert!(matches!(out[2].1, ResyncOutcome::Failed(_)));
    }
}
