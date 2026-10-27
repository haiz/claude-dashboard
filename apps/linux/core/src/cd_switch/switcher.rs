//! Swaps Claude Desktop's signed-in account (port of macOS `ClaudeDesktopSwitcher`, step for
//! step), using one independent grant per account, kept in its own vault so Desktop and
//! Claude Code never share a refresh-token chain.
//!
//! Desktop signs in as the account of its claude.ai `sessionKey` cookie, so `switch_to`
//! writes both the target's grant (`config.json`) and its session (`Cookies`).
//!
//! `capture_active`, `store`, `forget` and the write part of `switch_to` share one lock; the
//! provision call (a browser capture that can take minutes) runs outside it. `switch_to`
//! never writes while Desktop runs (Electron rewrites `config.json` on quit) and never
//! overwrites a live outgoing grant it could not first save.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{Map, Value};

use super::config::DesktopConfig;
use super::cookies::DesktopSession;
use super::process::DesktopProcess;
use crate::cc_switch::files::Vault;
use crate::cc_switch::model::{canonical, email_of, OAuthCredential, VaultEntry};

/// A dashboard account as the Desktop switcher sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopAccount {
    pub id: String,
    pub email: Option<String>,
    pub account_uuid: Option<String>,
    pub org_id: Option<String>,
    /// The claude.ai session key in plain text (the store keeps it sealed).
    pub session_key: Option<String>,
}

/// What the Desktop Switch button can do for one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAvailability {
    /// Claude Desktop is already signed in as this account.
    Active,
    Ready,
    /// No Desktop grant of its own yet: the Switch captures one through the browser.
    NotCaptured,
    /// The vault copy is past `refreshTokenExpiresAt`: the Switch captures a new one.
    LoginExpired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopSwitchError {
    /// Claude Desktop is running; the caller quits it first (with the user's consent).
    DesktopRunning,
    /// No usable vault grant and no way to capture one, or the capture named another account.
    NotCaptured,
    LoginExpired,
    /// The target has no claude.ai session key, which is what Desktop signs in with.
    NoSessionKey,
    /// Desktop holds a live login the dashboard cannot save (unknown account, or the save
    /// failed); overwriting it would discard its refresh token.
    UnknownOutgoingAccount { email: Option<String> },
    /// `config.json` or `Cookies` could not be written, or the target lacks an account or
    /// org uuid.
    Config,
    /// The written grant or session did not read back as the target's.
    VerifyFailed,
    /// The provisioner failed; the message is shown as-is.
    Provision(String),
}

impl DesktopSwitchError {
    /// Case name only, safe to log (no emails).
    pub fn log_name(&self) -> &'static str {
        match self {
            DesktopSwitchError::DesktopRunning => "desktopRunning",
            DesktopSwitchError::NotCaptured => "notCaptured",
            DesktopSwitchError::LoginExpired => "loginExpired",
            DesktopSwitchError::NoSessionKey => "noSessionKey",
            DesktopSwitchError::UnknownOutgoingAccount { .. } => "unknownOutgoingAccount",
            DesktopSwitchError::Config => "config",
            DesktopSwitchError::VerifyFailed => "verifyFailed",
            DesktopSwitchError::Provision(_) => "provision",
        }
    }
}

pub struct DesktopSwitcher {
    config: Box<dyn DesktopConfig + Send + Sync>,
    session: Box<dyn DesktopSession + Send + Sync>,
    vault: Box<dyn Vault + Send + Sync>,
    process: Box<dyn DesktopProcess + Send + Sync>,
    now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
    /// Never held across the provision call.
    lock: Mutex<()>,
}

impl DesktopSwitcher {
    pub fn new(
        config: Box<dyn DesktopConfig + Send + Sync>,
        session: Box<dyn DesktopSession + Send + Sync>,
        vault: Box<dyn Vault + Send + Sync>,
        process: Box<dyn DesktopProcess + Send + Sync>,
        now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
    ) -> Self {
        DesktopSwitcher { config, session, vault, process, now_ms, lock: Mutex::new(()) }
    }

    /// The real switcher: Claude Desktop's `config.json` and `Cookies` sealed with the key
    /// from its `Local State`, and the DPAPI vault in
    /// `%LOCALAPPDATA%\claude-dashboard\desktop-vault`. `None` when Desktop is not installed
    /// or its key cannot be read.
    #[cfg(windows)]
    pub fn live() -> Option<DesktopSwitcher> {
        use super::config::FileDesktopConfig;
        use super::cookies::FileDesktopSession;
        use super::process::Win32DesktopProcess;
        use crate::cc_switch::files::FileVault;

        let paths = super::paths::live()?;
        let key = crate::cookie::win::profile_key(&paths.local_state)?;
        let vault_dir = crate::store::usage_log_path().with_file_name("desktop-vault");
        Some(DesktopSwitcher::new(
            Box::new(FileDesktopConfig { path: paths.config, key }),
            Box::new(FileDesktopSession { path: paths.cookies, key, now_unix }),
            Box::new(FileVault::dpapi(vault_dir)),
            Box::new(Win32DesktopProcess),
            Box::new(|| (now_unix() * 1000.0) as i64),
        ))
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_running(&self) -> bool {
        self.process.is_running()
    }

    /// Quits Claude Desktop (blocks for seconds: call from a worker thread). Only after the
    /// user agreed to close it. True when no Desktop process remains.
    pub fn quit(&self) -> bool {
        self.process.quit()
    }

    /// Desktop's `lastKnownAccountUuid`, when it holds a readable grant.
    pub fn active_account_uuid(&self) -> Option<String> {
        self.config.read_active_grant().ok().flatten().map(|(uuid, _)| uuid)
    }

    // ---- Availability ----

    pub fn availability(&self, accounts: &[DesktopAccount]) -> HashMap<String, DesktopAvailability> {
        let active_uuid = self.active_account_uuid();
        accounts
            .iter()
            .map(|a| {
                let state = if active_uuid.is_some() && a.account_uuid == active_uuid {
                    DesktopAvailability::Active
                } else {
                    match self.vault.load(&a.id) {
                        None => DesktopAvailability::NotCaptured,
                        Some(e) if self.is_expired(&e) => DesktopAvailability::LoginExpired,
                        Some(_) => DesktopAvailability::Ready,
                    }
                };
                (a.id.clone(), state)
            })
            .collect()
    }

    // ---- Capture ----

    /// Opportunistic: saves Desktop's live grant to the vault of the dashboard account it names.
    pub fn capture_active(&self, accounts: &[DesktopAccount]) -> Result<(), String> {
        let _guard = self.locked();
        let grant = self.config.read_active_grant().map_err(|e| format!("{e:?}"))?;
        let Some((uuid, oauth)) = grant else { return Ok(()) };
        if oauth.is_blank() {
            return Ok(());
        }
        let Some(account) = account_by_uuid(&uuid, accounts) else { return Ok(()) };
        self.save_outgoing(&oauth, account)
    }

    // ---- Switch ----

    /// Moves Claude Desktop to `target`. Without a usable vault grant, `provision` (a browser
    /// capture; called outside the lock) mints one, which is validated and stored first.
    /// Blocks for as long as `provision` does: call from a worker thread.
    pub fn switch_to(
        &self,
        target: &DesktopAccount,
        accounts: &[DesktopAccount],
        provision: Option<&dyn Fn() -> Result<VaultEntry, DesktopSwitchError>>,
    ) -> Result<(), DesktopSwitchError> {
        let result = self.switch_unlogged(target, accounts, provision);
        if let Err(e) = &result {
            eprintln!("[cd-switch] switch -> {}: {}", target.id, e.log_name());
        }
        result
    }

    fn switch_unlogged(
        &self,
        target: &DesktopAccount,
        accounts: &[DesktopAccount],
        provision: Option<&dyn Fn() -> Result<VaultEntry, DesktopSwitchError>>,
    ) -> Result<(), DesktopSwitchError> {
        // Browser capture can take minutes: outside the lock, before anything is touched.
        self.guard_not_running()?;
        session_key(target)?;
        let entry = self.resolve_grant(target, provision)?;
        self.perform_write(&entry, target, accounts)
    }

    fn guard_not_running(&self) -> Result<(), DesktopSwitchError> {
        if self.process.is_running() {
            return Err(DesktopSwitchError::DesktopRunning);
        }
        Ok(())
    }

    /// The target's vault grant, or a fresh browser capture stored in the vault.
    fn resolve_grant(
        &self,
        target: &DesktopAccount,
        provision: Option<&dyn Fn() -> Result<VaultEntry, DesktopSwitchError>>,
    ) -> Result<VaultEntry, DesktopSwitchError> {
        let mut expired = false;
        if let Some(entry) = self.vault.load(&target.id) {
            if !entry.oauth.is_blank() && names(&entry, target) {
                if !self.is_expired(&entry) {
                    return Ok(entry);
                }
                expired = true;
            }
        }
        let (Some(provision), Some(_)) = (provision, target.email.as_ref()) else {
            return Err(if expired { DesktopSwitchError::LoginExpired } else { DesktopSwitchError::NotCaptured });
        };
        let entry = provision()?;
        if entry.oauth.is_blank() || self.is_expired(&entry) || !names(&entry, target) {
            return Err(DesktopSwitchError::NotCaptured);
        }
        self.store(&target.id, &entry)
            .map_err(|e| DesktopSwitchError::Provision(format!("Could not save the new Claude Desktop login: {e}")))?;
        Ok(entry)
    }

    /// Locked: no capture or second switch interleaves between reading the outgoing grant
    /// and writing the target's.
    fn perform_write(
        &self,
        entry: &VaultEntry,
        target: &DesktopAccount,
        accounts: &[DesktopAccount],
    ) -> Result<(), DesktopSwitchError> {
        let _guard = self.locked();

        // Re-check under the lock: Desktop may have launched during the browser capture.
        self.guard_not_running()?;
        let session_key = session_key(target)?;
        let (Some(account_uuid), Some(org_uuid)) = (target.account_uuid.as_deref(), target.org_id.as_deref()) else {
            return Err(DesktopSwitchError::Config);
        };

        let outgoing = self
            .config
            .read_active_grant()
            .map_err(|_| DesktopSwitchError::UnknownOutgoingAccount { email: None })?;
        // Already holding a live grant for the target: keep it (its chain may be newer than
        // the vault's), but the session cookie below may still name another account.
        let has_target_grant = outgoing.as_ref().is_some_and(|(uuid, o)| !o.is_blank() && uuid == account_uuid);
        if let Some((uuid, oauth)) = outgoing.as_ref().filter(|(_, o)| !o.is_blank() && !has_target_grant) {
            let account = account_by_uuid(uuid, accounts)
                .ok_or(DesktopSwitchError::UnknownOutgoingAccount { email: None })?;
            self.save_outgoing(oauth, account)
                .map_err(|_| DesktopSwitchError::UnknownOutgoingAccount { email: account.email.clone() })?;
        }

        if !has_target_grant {
            self.config
                .write_grant(&entry.oauth, account_uuid, org_uuid)
                .map_err(|_| DesktopSwitchError::Config)?;
            let verified = matches!(self.config.read_active_grant(),
                Ok(Some((uuid, o))) if uuid == account_uuid && o.refresh_token == entry.oauth.refresh_token);
            if !verified {
                return Err(DesktopSwitchError::VerifyFailed);
            }
        }

        // Desktop signs in as the account of this cookie; the grant alone is ignored.
        self.session.write_session(session_key, org_uuid).map_err(|_| DesktopSwitchError::Config)?;
        if self.session.read_session_key().as_deref() != Some(session_key) {
            return Err(DesktopSwitchError::VerifyFailed);
        }
        Ok(())
    }

    // ---- Store / forget ----

    /// Saves a grant the provisioner minted for an account.
    pub fn store(&self, id: &str, e: &VaultEntry) -> Result<(), String> {
        let _guard = self.locked();
        self.vault.save(id, e)
    }

    /// Drops a removed account's vault copy, so no refresh token outlives its account.
    pub fn forget(&self, id: &str) {
        let _guard = self.locked();
        self.vault.delete(id);
    }

    // ---- Helpers ----

    /// Caller holds the lock. Keeps the stored `oauthAccount` when there is one.
    fn save_outgoing(&self, oauth: &OAuthCredential, account: &DesktopAccount) -> Result<(), String> {
        let existing = self.vault.load(&account.id);
        if existing.as_ref().is_some_and(|e| &e.oauth == oauth) {
            return Ok(());
        }
        let oauth_account = match existing {
            Some(e) => e.oauth_account,
            None => {
                let mut info = Map::new();
                info.insert("uuid".into(), Value::from(account.account_uuid.clone().unwrap_or_default()));
                if let Some(email) = &account.email {
                    info.insert("emailAddress".into(), Value::from(email.clone()));
                }
                canonical(&Value::Object(info)).unwrap_or_default()
            }
        };
        self.vault.save(&account.id, &VaultEntry { oauth: oauth.clone(), oauth_account })
    }

    fn is_expired(&self, e: &VaultEntry) -> bool {
        e.oauth.refresh_expires_at_ms.is_some_and(|d| d <= (self.now_ms)())
    }
}

/// False only when the entry names an email other than the target's.
fn names(entry: &VaultEntry, target: &DesktopAccount) -> bool {
    match (email_of(&entry.oauth_account), target.email.as_deref()) {
        (Some(email), Some(expected)) => email.eq_ignore_ascii_case(expected),
        _ => true,
    }
}

/// The claude.ai session Desktop is moved to; checked before any browser capture.
fn session_key(target: &DesktopAccount) -> Result<&str, DesktopSwitchError> {
    target.session_key.as_deref().filter(|k| !k.is_empty()).ok_or(DesktopSwitchError::NoSessionKey)
}

fn account_by_uuid<'a>(uuid: &str, accounts: &'a [DesktopAccount]) -> Option<&'a DesktopAccount> {
    accounts.iter().find(|a| a.account_uuid.as_deref() == Some(uuid))
}

#[cfg(windows)]
fn now_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cd_switch::config::ConfigError;
    use serde_json::json;
    use std::sync::Arc;

    const NOW: i64 = 1_800_000_000_000;

    #[derive(Default)]
    struct State {
        grant: Option<(String, OAuthCredential)>,
        /// Simulates a present grant that will not decrypt.
        config_unreadable: bool,
        config_writes: Vec<(OAuthCredential, String, String)>,
        config_fail_write: bool,
        session_key: Option<String>,
        session_writes: Vec<(String, String)>,
        session_fail_write: bool,
        /// Simulates a session write that does not read back.
        session_drop_writes: bool,
        vault: HashMap<String, VaultEntry>,
        vault_fail_save: bool,
        running: bool,
        quit_calls: u32,
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<State>>);

    impl Fake {
        fn s(&self) -> std::sync::MutexGuard<'_, State> {
            self.0.lock().unwrap()
        }
    }

    impl DesktopConfig for Fake {
        fn read_active_grant(&self) -> Result<Option<(String, OAuthCredential)>, ConfigError> {
            let s = self.s();
            if s.config_unreadable {
                return Err(ConfigError::Unreadable);
            }
            Ok(s.grant.clone())
        }
        fn write_grant(&self, oauth: &OAuthCredential, account_uuid: &str, org_uuid: &str) -> Result<(), ConfigError> {
            let mut s = self.s();
            if s.config_fail_write {
                return Err(ConfigError::Unreadable);
            }
            s.config_writes.push((oauth.clone(), account_uuid.into(), org_uuid.into()));
            s.grant = Some((account_uuid.into(), oauth.clone()));
            Ok(())
        }
    }

    impl DesktopSession for Fake {
        fn read_session_key(&self) -> Option<String> {
            self.s().session_key.clone()
        }
        fn write_session(&self, session_key: &str, org_uuid: &str) -> Result<(), String> {
            let mut s = self.s();
            if s.session_fail_write {
                return Err("fail".into());
            }
            s.session_writes.push((session_key.into(), org_uuid.into()));
            if !s.session_drop_writes {
                s.session_key = Some(session_key.into());
            }
            Ok(())
        }
    }

    impl Vault for Fake {
        fn load(&self, id: &str) -> Option<VaultEntry> {
            self.s().vault.get(id).cloned()
        }
        fn save(&self, id: &str, e: &VaultEntry) -> Result<(), String> {
            let mut s = self.s();
            if s.vault_fail_save {
                return Err("encryption failed".into());
            }
            s.vault.insert(id.into(), e.clone());
            Ok(())
        }
        fn delete(&self, id: &str) {
            self.s().vault.remove(id);
        }
    }

    impl DesktopProcess for Fake {
        fn is_running(&self) -> bool {
            self.s().running
        }
        fn quit(&self) -> bool {
            self.s().quit_calls += 1;
            true
        }
    }

    fn switcher(f: &Fake) -> DesktopSwitcher {
        DesktopSwitcher::new(
            Box::new(f.clone()),
            Box::new(f.clone()),
            Box::new(f.clone()),
            Box::new(f.clone()),
            Box::new(|| NOW),
        )
    }

    fn account(name: &str) -> DesktopAccount {
        DesktopAccount {
            id: format!("id-{name}"),
            email: Some(format!("{name}@x.co")),
            account_uuid: Some(format!("uuid-{name}")),
            org_id: Some(format!("org-{name}")),
            session_key: Some(format!("sk-{name}")),
        }
    }

    fn a() -> DesktopAccount {
        account("a")
    }
    fn b() -> DesktopAccount {
        account("b")
    }
    fn accounts() -> Vec<DesktopAccount> {
        vec![a(), b()]
    }

    fn oauth_with(refresh: &str, refresh_expires_ms: Option<i64>) -> OAuthCredential {
        let mut v = json!({"accessToken": format!("acc-{refresh}"), "refreshToken": refresh,
                           "expiresAt": 1_900_000_000_000i64});
        if let Some(ms) = refresh_expires_ms {
            v["refreshTokenExpiresAt"] = json!(ms);
        }
        OAuthCredential::from_value(&v).unwrap()
    }

    fn oauth(refresh: &str) -> OAuthCredential {
        oauth_with(refresh, None)
    }

    fn entry_with(refresh: &str, email: &str, refresh_expires_ms: Option<i64>) -> VaultEntry {
        VaultEntry {
            oauth: oauth_with(refresh, refresh_expires_ms),
            oauth_account: canonical(&json!({"emailAddress": email})).unwrap(),
        }
    }

    fn entry(refresh: &str, email: &str) -> VaultEntry {
        entry_with(refresh, email, None)
    }

    const PAST: i64 = NOW - 1000;

    fn put(f: &Fake, id: &str, e: VaultEntry) {
        f.s().vault.insert(id.into(), e);
    }

    fn grant(f: &Fake, uuid: &str, o: OAuthCredential) {
        f.s().grant = Some((uuid.into(), o));
    }

    #[test]
    fn switch_refuses_while_desktop_running() {
        let f = Fake::default();
        f.s().running = true;
        put(&f, "id-a", entry("ra", "a@x.co"));
        grant(&f, "uuid-b", oauth("rb"));
        assert_eq!(switcher(&f).switch_to(&a(), &accounts(), None), Err(DesktopSwitchError::DesktopRunning));
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert!(!s.vault.contains_key("id-b"));
        assert_eq!(s.quit_calls, 0);
    }

    #[test]
    fn switch_captures_outgoing_before_overwrite() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        put(&f, "id-b", entry("rb-old", "b@x.co"));
        grant(&f, "uuid-b", oauth("rb-fresh"));
        switcher(&f).switch_to(&a(), &accounts(), None).unwrap();
        let s = f.s();
        assert_eq!(s.vault["id-b"].oauth.refresh_token, "rb-fresh");
        assert_eq!(email_of(&s.vault["id-b"].oauth_account).as_deref(), Some("b@x.co"));
        assert_eq!(s.config_writes.len(), 1);
        assert_eq!(s.grant.as_ref().unwrap().0, "uuid-a");
    }

    #[test]
    fn switch_captures_outgoing_with_no_prior_vault_entry() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        grant(&f, "uuid-b", oauth("rb-fresh"));
        switcher(&f).switch_to(&a(), &accounts(), None).unwrap();
        let s = f.s();
        assert_eq!(s.vault["id-b"].oauth.refresh_token, "rb-fresh");
        assert_eq!(email_of(&s.vault["id-b"].oauth_account).as_deref(), Some("b@x.co"));
        assert_eq!(s.vault["id-b"].oauth_account, r#"{"emailAddress":"b@x.co","uuid":"uuid-b"}"#);
    }

    #[test]
    fn switch_refuses_when_outgoing_cannot_be_saved() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        grant(&f, "uuid-b", oauth("rb"));
        f.s().vault_fail_save = true;
        assert_eq!(
            switcher(&f).switch_to(&a(), &accounts(), None),
            Err(DesktopSwitchError::UnknownOutgoingAccount { email: Some("b@x.co".into()) })
        );
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert_eq!(s.grant.as_ref().unwrap().1.refresh_token, "rb");
    }

    #[test]
    fn switch_guards_unknown_outgoing_account() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        grant(&f, "uuid-stranger", oauth("rs"));
        assert_eq!(
            switcher(&f).switch_to(&a(), &accounts(), None),
            Err(DesktopSwitchError::UnknownOutgoingAccount { email: None })
        );
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert_eq!(s.grant.as_ref().unwrap().0, "uuid-stranger");
        assert_eq!(s.grant.as_ref().unwrap().1.refresh_token, "rs");
    }

    #[test]
    fn switch_refuses_undecryptable_outgoing_grant() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        f.s().config_unreadable = true;
        assert_eq!(
            switcher(&f).switch_to(&a(), &accounts(), None),
            Err(DesktopSwitchError::UnknownOutgoingAccount { email: None })
        );
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert!(s.session_writes.is_empty());
    }

    #[test]
    fn unreadable_grant_reads_as_unknown() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        f.s().config_unreadable = true;
        let sw = switcher(&f);
        assert_eq!(sw.active_account_uuid(), None);
        assert_eq!(sw.availability(&accounts())["id-a"], DesktopAvailability::Ready);
        assert!(sw.capture_active(&accounts()).is_err());
        assert!(!f.s().vault.contains_key("id-b"));
    }

    #[test]
    fn switch_skips_blank_or_absent_outgoing() {
        let f = Fake::default();
        let sw = switcher(&f);
        put(&f, "id-a", entry("ra", "a@x.co"));
        sw.switch_to(&a(), &accounts(), None).unwrap();
        assert_eq!(f.s().config_writes.len(), 1);

        let blank = OAuthCredential::from_value(&json!({"accessToken": "", "refreshToken": ""})).unwrap();
        grant(&f, "uuid-stranger", blank);
        sw.switch_to(&a(), &accounts(), None).unwrap();
        assert_eq!(f.s().config_writes.len(), 2);
        assert!(!f.s().vault.contains_key("id-b"));
    }

    #[test]
    fn switch_uses_vault_grant_when_present() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        let provisioned = std::cell::Cell::new(false);
        let provision = || -> Result<VaultEntry, DesktopSwitchError> {
            provisioned.set(true);
            Err(DesktopSwitchError::Provision("cancelled".into()))
        };
        switcher(&f).switch_to(&a(), &accounts(), Some(&provision)).unwrap();
        assert!(!provisioned.get());
        let s = f.s();
        let (o, uuid, org) = &s.config_writes[0];
        assert_eq!(o.refresh_token, "ra");
        assert_eq!(uuid, "uuid-a");
        assert_eq!(org, "org-a");
    }

    #[test]
    fn switch_browser_captures_when_missing() {
        let f = Fake::default();
        let calls = std::cell::Cell::new(0);
        let provision = || {
            calls.set(calls.get() + 1);
            Ok(entry("r-new", "a@x.co"))
        };
        switcher(&f).switch_to(&a(), &accounts(), Some(&provision)).unwrap();
        assert_eq!(calls.get(), 1);
        let s = f.s();
        assert_eq!(s.vault["id-a"].oauth.refresh_token, "r-new");
        assert_eq!(s.config_writes[0].0.refresh_token, "r-new");
    }

    #[test]
    fn switch_browser_captures_when_vault_grant_expired() {
        let f = Fake::default();
        put(&f, "id-a", entry_with("ra-dead", "a@x.co", Some(PAST)));
        let provision = || Ok(entry("r-new", "a@x.co"));
        switcher(&f).switch_to(&a(), &accounts(), Some(&provision)).unwrap();
        assert_eq!(f.s().config_writes[0].0.refresh_token, "r-new");
    }

    #[test]
    fn switch_without_vault_grant_or_provisioner_is_not_captured() {
        let f = Fake::default();
        let sw = switcher(&f);
        assert_eq!(sw.switch_to(&a(), &accounts(), None), Err(DesktopSwitchError::NotCaptured));
        put(&f, "id-a", entry_with("ra-dead", "a@x.co", Some(PAST)));
        assert_eq!(sw.switch_to(&a(), &accounts(), None), Err(DesktopSwitchError::LoginExpired));
        assert!(f.s().config_writes.is_empty());
    }

    #[test]
    fn provision_failure_leaves_config_untouched() {
        let f = Fake::default();
        grant(&f, "uuid-b", oauth("rb"));
        let provision = || Err(DesktopSwitchError::Provision("Sign-in was cancelled.".into()));
        assert_eq!(
            switcher(&f).switch_to(&a(), &accounts(), Some(&provision)),
            Err(DesktopSwitchError::Provision("Sign-in was cancelled.".into()))
        );
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert!(!s.vault.contains_key("id-b"));
    }

    #[test]
    fn provisioned_grant_for_another_email_is_not_captured() {
        let f = Fake::default();
        let provision = || Ok(entry("r-new", "b@x.co"));
        assert_eq!(switcher(&f).switch_to(&a(), &accounts(), Some(&provision)), Err(DesktopSwitchError::NotCaptured));
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert!(s.vault.is_empty());
    }

    #[test]
    fn desktop_starting_during_browser_capture_is_refused() {
        let f = Fake::default();
        let provision = || {
            f.s().running = true;
            Ok(entry("r-new", "a@x.co"))
        };
        assert_eq!(switcher(&f).switch_to(&a(), &accounts(), Some(&provision)), Err(DesktopSwitchError::DesktopRunning));
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert!(s.session_writes.is_empty());
    }

    #[test]
    fn switch_needs_account_and_org_uuid() {
        let f = Fake::default();
        let mut bare = a();
        bare.org_id = None;
        put(&f, "id-a", entry("ra", "a@x.co"));
        assert_eq!(switcher(&f).switch_to(&bare, &[bare.clone(), b()], None), Err(DesktopSwitchError::Config));
        assert!(f.s().config_writes.is_empty());
    }

    #[test]
    fn write_failure_is_config_error() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        f.s().config_fail_write = true;
        assert_eq!(switcher(&f).switch_to(&a(), &accounts(), None), Err(DesktopSwitchError::Config));
    }

    #[test]
    fn already_active_live_grant_keeps_token_but_still_writes_session() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        grant(&f, "uuid-a", oauth("ra-live"));
        f.s().session_key = Some("sk-b".into());
        switcher(&f).switch_to(&a(), &accounts(), None).unwrap();
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert_eq!(s.session_key.as_deref(), Some("sk-a"));
    }

    // Desktop signs in as the account of its claude.ai `sessionKey` cookie, not the token
    // cache: a token-only write reopened Desktop as the old account (2026-10-27, macOS).
    #[test]
    fn switch_writes_target_session_cookie() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        put(&f, "id-b", entry("rb", "b@x.co"));
        grant(&f, "uuid-b", oauth("rb"));
        f.s().session_key = Some("sk-b".into());
        switcher(&f).switch_to(&a(), &accounts(), None).unwrap();
        let s = f.s();
        assert_eq!(s.session_writes, vec![("sk-a".to_string(), "org-a".to_string())]);
        assert_eq!(s.grant.as_ref().unwrap().0, "uuid-a");
    }

    #[test]
    fn switch_without_session_key_is_refused_before_anything() {
        let f = Fake::default();
        let mut bare = a();
        bare.session_key = None;
        grant(&f, "uuid-b", oauth("rb"));
        let provisioned = std::cell::Cell::new(false);
        let provision = || {
            provisioned.set(true);
            Ok(entry("r-new", "a@x.co"))
        };
        let sw = switcher(&f);
        assert_eq!(sw.switch_to(&bare, &[bare.clone(), b()], Some(&provision)), Err(DesktopSwitchError::NoSessionKey));
        bare.session_key = Some(String::new());
        assert_eq!(sw.switch_to(&bare, &[bare.clone(), b()], Some(&provision)), Err(DesktopSwitchError::NoSessionKey));
        assert!(!provisioned.get());
        let s = f.s();
        assert!(s.config_writes.is_empty());
        assert!(s.session_writes.is_empty());
    }

    #[test]
    fn session_write_failure_is_config_error() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        f.s().session_fail_write = true;
        assert_eq!(switcher(&f).switch_to(&a(), &accounts(), None), Err(DesktopSwitchError::Config));
    }

    #[test]
    fn session_that_does_not_read_back_fails_verify() {
        let f = Fake::default();
        put(&f, "id-a", entry("ra", "a@x.co"));
        f.s().session_drop_writes = true;
        assert_eq!(switcher(&f).switch_to(&a(), &accounts(), None), Err(DesktopSwitchError::VerifyFailed));
    }

    #[test]
    fn availability_not_captured() {
        let f = Fake::default();
        let map = switcher(&f).availability(&accounts());
        assert_eq!(map["id-a"], DesktopAvailability::NotCaptured);
        assert_eq!(map["id-b"], DesktopAvailability::NotCaptured);
    }

    #[test]
    fn availability_active_ready_expired() {
        let f = Fake::default();
        let sw = switcher(&f);
        grant(&f, "uuid-a", oauth("ra"));
        put(&f, "id-b", entry_with("rb", "b@x.co", Some(PAST)));
        let expected: HashMap<String, DesktopAvailability> = [
            ("id-a".to_string(), DesktopAvailability::Active),
            ("id-b".to_string(), DesktopAvailability::LoginExpired),
        ]
        .into();
        assert_eq!(sw.availability(&accounts()), expected);
        put(&f, "id-b", entry("rb", "b@x.co"));
        assert_eq!(sw.availability(&accounts())["id-b"], DesktopAvailability::Ready);
        assert_eq!(sw.active_account_uuid().as_deref(), Some("uuid-a"));
    }

    #[test]
    fn capture_active_saves_dashboard_account_only() {
        let f = Fake::default();
        let sw = switcher(&f);
        grant(&f, "uuid-b", oauth("rb"));
        sw.capture_active(&accounts()).unwrap();
        assert_eq!(f.s().vault["id-b"].oauth.refresh_token, "rb");
        grant(&f, "uuid-stranger", oauth("rs"));
        sw.capture_active(&accounts()).unwrap();
        assert_eq!(f.s().vault.len(), 1);
    }

    #[test]
    fn store_forget_and_passthroughs() {
        let f = Fake::default();
        let sw = switcher(&f);
        sw.store("id-a", &entry("ra", "a@x.co")).unwrap();
        assert!(f.s().vault.contains_key("id-a"));
        sw.forget("id-a");
        assert!(!f.s().vault.contains_key("id-a"));
        assert!(!sw.is_running());
        f.s().running = true;
        assert!(sw.is_running());
        assert!(sw.quit());
        assert_eq!(f.s().quit_calls, 1);
        assert_eq!(sw.active_account_uuid(), None);
    }

    #[test]
    fn log_names_are_case_names_only() {
        assert_eq!(DesktopSwitchError::UnknownOutgoingAccount { email: Some("b@x.co".into()) }.log_name(), "unknownOutgoingAccount");
        assert_eq!(DesktopSwitchError::Provision("x".into()).log_name(), "provision");
        assert_eq!(DesktopSwitchError::DesktopRunning.log_name(), "desktopRunning");
    }
}
