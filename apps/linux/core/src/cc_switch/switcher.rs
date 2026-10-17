//! Keeps every account's Claude Code login alive and swaps it into Claude Code's config
//! directory (port of macOS `ClaudeCodeSwitcher`, rule for rule).
//!
//! `capture` runs on every refresh and copies the active credential into the vault, because
//! Claude Code rotates the refresh token on each refresh and an older copy is dead.
//! `switch_to` saves the active account, then writes the target's credential and
//! `oauthAccount`. `capture`, `store`, `forget` and the write part of `switch_to` share one
//! lock, so a capture never reads the config of one account and the credential of another.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use super::files::{ConfigFile, CredentialSlot, SlotError, Vault};
use super::model::{email_of, OAuthCredential, VaultEntry};

/// A token this close to expiry, or this little past it, may be refreshing right now.
pub const REFRESH_LEAD_MS: i64 = 300_000;
pub const REFRESH_GRACE_MS: i64 = 60_000;
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
pub const MAX_POLLS: usize = 10;
/// Switch attempts when a refresh keeps landing between the slot read and its write.
pub const MAX_WRITE_ATTEMPTS: usize = 3;

/// A dashboard account as the switcher sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchAccount {
    pub id: String,
    pub email: Option<String>,
}

/// What the Switch button can do for one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// Claude Code already uses this account.
    Active,
    Ready,
    /// The vault has no copy: sign in once.
    NotCaptured,
    /// The vault copy is past `refreshTokenExpiresAt`.
    LoginExpired,
    /// Active, but Claude Code blanked the credential after a failed refresh.
    NeedsLogin,
}

impl Availability {
    /// Tooltip for the Switch control (macOS `switchHelp`); `None` when it is hidden.
    pub fn help(self) -> Option<&'static str> {
        match self {
            Availability::Active => None,
            Availability::Ready => Some("Use this account in Claude Code"),
            Availability::NotCaptured => Some("Sign this account in (opens its browser profile), then switch"),
            Availability::LoginExpired => Some("Login expired (~30 days); sign in again in the browser, then switch"),
            Availability::NeedsLogin => Some("Claude Code lost this login. Run /login with this account"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchError {
    /// The vault has no copy of the target, or its copy names another email.
    NotCaptured,
    LoginExpired,
    /// Claude Code is signed in as an account the dashboard does not know.
    ActiveAccountNotInDashboard(String),
    /// Claude Code holds a live login but `.claude.json` names no account.
    ActiveAccountUnknown,
    /// The credentials file could not be read or updated, or kept changing; it is as the
    /// switch found it.
    CredentialsFile,
    /// `.claude.json` could not be written; the previous credential was restored.
    Config,
    /// The credentials file may hold the target while `.claude.json` names the previous
    /// account, and the previous credential could not be restored.
    RollbackFailed,
    VerifyFailed,
}

impl SwitchError {
    /// Case name only, safe to log.
    pub fn log_name(&self) -> &'static str {
        match self {
            SwitchError::NotCaptured => "notCaptured",
            SwitchError::LoginExpired => "loginExpired",
            SwitchError::ActiveAccountNotInDashboard(_) => "activeAccountNotInDashboard",
            SwitchError::ActiveAccountUnknown => "activeAccountUnknown",
            SwitchError::CredentialsFile => "credentialsFile",
            SwitchError::Config => "config",
            SwitchError::RollbackFailed => "rollbackFailed",
            SwitchError::VerifyFailed => "verifyFailed",
        }
    }

    /// The message shown after a failed switch (macOS `message(for:target:)`, with the
    /// Keychain read as the credentials file and `~/.claude.json` as its Windows path).
    pub fn message(&self, target_name: &str) -> String {
        let name = target_name;
        match self {
            SwitchError::NotCaptured => {
                format!("Run /login once in Claude Code with {name} so the dashboard can keep its login.")
            }
            SwitchError::LoginExpired => {
                format!("The saved Claude Code login for {name} has expired. Run /login once with it.")
            }
            SwitchError::ActiveAccountNotInDashboard(email) => format!(
                "Claude Code is signed in as {email}, which is not in the dashboard. Add it first, or its login would be lost."
            ),
            SwitchError::CredentialsFile => "Could not update Claude Code's credentials file. Nothing was changed.".into(),
            SwitchError::Config => {
                "Could not update %USERPROFILE%\\.claude.json. The previous account was restored.".into()
            }
            SwitchError::VerifyFailed => "The switch could not be confirmed. Check /status in Claude Code.".into(),
            SwitchError::ActiveAccountUnknown => "Claude Code is signed in to an account that %USERPROFILE%\\.claude.json does not name. \
                 Run /login in Claude Code with that account first, or its login would be lost."
                .into(),
            SwitchError::RollbackFailed => format!(
                "Claude Code's credentials file may now hold {name}'s login, but %USERPROFILE%\\.claude.json still names \
                 the previous account. Run /login in Claude Code to fix it."
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture {
    NoActiveAccount,
    Blanked(String),
    Unchanged(String),
    Saved(String),
    /// The credentials file holds another dashboard account's login while `.claude.json`
    /// names this account; nothing was saved.
    Mismatch(String),
}

pub struct Switcher {
    slot: Box<dyn CredentialSlot + Send + Sync>,
    vault: Box<dyn Vault + Send + Sync>,
    config: Box<dyn ConfigFile + Send + Sync>,
    now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
    lock: Mutex<()>,
}

impl Switcher {
    pub fn new(
        slot: Box<dyn CredentialSlot + Send + Sync>,
        vault: Box<dyn Vault + Send + Sync>,
        config: Box<dyn ConfigFile + Send + Sync>,
        now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
        sleep: Box<dyn Fn(Duration) + Send + Sync>,
    ) -> Self {
        Switcher { slot, vault, config, now_ms, sleep, lock: Mutex::new(()) }
    }

    /// The real switcher for this user: Claude Code's credentials file and `.claude.json`
    /// (following `CLAUDE_CONFIG_DIR`), and the DPAPI vault in
    /// `%LOCALAPPDATA%\claude-dashboard\cc-vault`.
    #[cfg(windows)]
    pub fn live() -> Self {
        use super::files::{FileConfig, FileSlot, FileVault};
        let vault_dir = crate::store::usage_log_path().with_file_name("cc-vault");
        Switcher::new(
            Box::new(FileSlot { path: crate::claude_code::credentials_path() }),
            Box::new(FileVault::dpapi(vault_dir)),
            Box::new(FileConfig { path: crate::claude_code::claude_json_path() }),
            Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            }),
            Box::new(std::thread::sleep),
        )
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- Capture ----

    pub fn capture(&self, accounts: &[SwitchAccount]) -> Result<Capture, SlotError> {
        let _guard = self.locked();
        self.capture_unlocked(accounts)
    }

    fn capture_unlocked(&self, accounts: &[SwitchAccount]) -> Result<Capture, SlotError> {
        let Some(account_json) = self.config.read_oauth_account() else { return Ok(Capture::NoActiveAccount) };
        let Some(account) = email_of(&account_json).and_then(|e| match_email(&e, accounts)) else {
            return Ok(Capture::NoActiveAccount);
        };
        let oauth = match self.slot.read_oauth()? {
            Some(c) if !c.is_blank() => c,
            _ => return Ok(Capture::Blanked(account.id.clone())),
        };
        if let Some((owner, _)) = self.other_owner(&oauth, &account.id, accounts) {
            log_mismatch(&owner.id, &account.id);
            return Ok(Capture::Mismatch(account.id.clone()));
        }
        let entry = VaultEntry { oauth, oauth_account: account_json };
        if self.vault.load(&account.id).as_ref() == Some(&entry) {
            return Ok(Capture::Unchanged(account.id.clone()));
        }
        self.vault.save(&account.id, &entry).map_err(SlotError::Io)?;
        Ok(Capture::Saved(account.id.clone()))
    }

    // ---- Store / forget ----

    /// Saves a login the provisioner minted for `id` (not the active account). Serialized with
    /// capture and switch so it never interleaves with a credential write.
    pub fn store(&self, id: &str, entry: &VaultEntry) -> Result<(), String> {
        let _guard = self.locked();
        self.vault.save(id, entry)
    }

    /// Drops a removed account's vault copy, so no refresh token outlives its account.
    pub fn forget(&self, id: &str) {
        let _guard = self.locked();
        self.vault.delete(id);
    }

    // ---- Availability ----

    pub fn availability(&self, accounts: &[SwitchAccount]) -> HashMap<String, Availability> {
        let active_id = self
            .config
            .read_oauth_account()
            .and_then(|j| email_of(&j))
            .and_then(|e| match_email(&e, accounts).map(|a| a.id.clone()));
        let active_blank = !matches!(self.slot.read_oauth(), Ok(Some(c)) if !c.is_blank());
        accounts
            .iter()
            .map(|a| {
                let state = if Some(&a.id) == active_id.as_ref() {
                    if active_blank { Availability::NeedsLogin } else { Availability::Active }
                } else {
                    match self.vault.load(&a.id) {
                        None => Availability::NotCaptured,
                        Some(e) if self.is_expired(&e) => Availability::LoginExpired,
                        Some(_) => Availability::Ready,
                    }
                };
                (a.id.clone(), state)
            })
            .collect()
    }

    // ---- Switch ----

    /// Switches Claude Code to `target`. `Ok(false)` when it already uses it. Blocks while
    /// waiting for an imminent token refresh (up to 30 s): call from a worker thread.
    pub fn switch_to(&self, target: &SwitchAccount, accounts: &[SwitchAccount]) -> Result<bool, SwitchError> {
        // Outside the lock: a capture must stay possible while we wait for the refresh to land.
        self.wait_for_imminent_refresh();
        let _guard = self.locked();

        let active_json = self.config.read_oauth_account();
        let active_email = active_json.as_deref().and_then(email_of);
        let active_account = active_email.as_deref().and_then(|e| match_email(e, accounts));
        let from = active_account.map_or("unknown", |a| a.id.as_str());
        let result = self.switch_locked(target, accounts, active_json.as_deref(), active_email.as_deref(), active_account);
        match &result {
            Ok(switched) => eprintln!("[cc-switch] switch {from} -> {}: {}", target.id, if *switched { "switched" } else { "no-op" }),
            Err(e) => eprintln!("[cc-switch] switch {from} -> {}: {}", target.id, e.log_name()),
        }
        result
    }

    fn switch_locked(
        &self,
        target: &SwitchAccount,
        accounts: &[SwitchAccount],
        active_json: Option<&str>,
        active_email: Option<&str>,
        active_account: Option<&SwitchAccount>,
    ) -> Result<bool, SwitchError> {
        if active_email.is_some_and(|e| match_email(e, std::slice::from_ref(target)).is_some()) {
            return Ok(false);
        }
        let entry = self.vault.load(&target.id).ok_or(SwitchError::NotCaptured)?;
        let vault_email = email_of(&entry.oauth_account);
        if vault_email.as_deref().and_then(|e| match_email(e, std::slice::from_ref(target))).is_none() {
            eprintln!("[cc-switch] switch refused: vault copy of {} names another email", target.id);
            return Err(SwitchError::NotCaptured);
        }
        if self.is_expired(&entry) {
            return Err(SwitchError::LoginExpired);
        }

        // A session refreshing the active account between our read and write would lose its
        // newest token: the write is compare-and-swap, and a lost race starts over.
        let mut attempt = 1;
        loop {
            match self.install_target(&entry, accounts, active_json, active_account, active_email) {
                Ok(previous) => {
                    self.write_config(&entry, previous.as_ref())?;
                    break;
                }
                Err(Install::Changed) => {
                    if attempt >= MAX_WRITE_ATTEMPTS {
                        return Err(SwitchError::CredentialsFile);
                    }
                    attempt += 1;
                    eprintln!("[cc-switch] credentials changed during switch, retrying (attempt {attempt})");
                }
                Err(Install::Failed(e)) => return Err(e),
            }
        }

        let slot_ok = matches!(self.slot.read_oauth(), Ok(Some(c)) if c == entry.oauth);
        if !slot_ok || self.config.read_oauth_account().as_deref() != Some(entry.oauth_account.as_str()) {
            return Err(SwitchError::VerifyFailed);
        }
        Ok(true)
    }

    /// One read of the slot: saves it to the active account (unless it belongs to another
    /// account) and writes the target's credential over exactly that value. Returns the
    /// credential it replaced (the rollback copy).
    fn install_target(
        &self,
        entry: &VaultEntry,
        accounts: &[SwitchAccount],
        active_json: Option<&str>,
        active_account: Option<&SwitchAccount>,
        active_email: Option<&str>,
    ) -> Result<Option<OAuthCredential>, Install> {
        let previous = self.slot.read_oauth().map_err(|_| Install::Failed(SwitchError::CredentialsFile))?;
        let live = previous.as_ref().filter(|c| !c.is_blank());
        if live.is_some() && active_account.is_none() {
            return Err(Install::Failed(match active_email {
                Some(e) => SwitchError::ActiveAccountNotInDashboard(e.to_string()),
                None => SwitchError::ActiveAccountUnknown,
            }));
        }
        if let (Some(live), Some(active), Some(json)) = (live, active_account, active_json) {
            if let Some((owner, owner_entry)) = self.other_owner(live, &active.id, accounts) {
                // Not the active account's credential: never file it under that account.
                // Overwriting it is safe only if its owner's vault already holds it.
                log_mismatch(&owner.id, &active.id);
                if &owner_entry.oauth != live {
                    return Err(Install::Failed(SwitchError::ActiveAccountUnknown));
                }
            } else {
                let current = VaultEntry { oauth: live.clone(), oauth_account: json.to_string() };
                if self.vault.load(&active.id).as_ref() != Some(&current) {
                    self.vault.save(&active.id, &current).map_err(|_| Install::Failed(SwitchError::CredentialsFile))?;
                }
            }
        }
        match self.slot.write_oauth(&entry.oauth, previous.as_ref()) {
            Ok(()) => Ok(previous),
            Err(SlotError::ChangedSinceRead) => Err(Install::Changed),
            Err(SlotError::Unreadable) => Err(Install::Failed(SwitchError::CredentialsFile)),
            Err(SlotError::Io(_)) => Err(Install::Failed(self.undo_failed_write(&entry.oauth, previous.as_ref()))),
        }
    }

    /// A slot write that failed may still have changed the file. Restores `previous` if the
    /// target's credential landed; reports `CredentialsFile` ("nothing changed") only when a
    /// re-read shows the file holds `previous`.
    fn undo_failed_write(&self, written: &OAuthCredential, previous: Option<&OAuthCredential>) -> SwitchError {
        let Ok(current) = self.slot.read_oauth() else { return SwitchError::RollbackFailed };
        if current.as_ref() == previous {
            return SwitchError::CredentialsFile;
        }
        let (Some(previous), true) = (previous, current.as_ref() == Some(written)) else {
            return SwitchError::RollbackFailed;
        };
        let _ = self.slot.write_oauth(previous, Some(written));
        if matches!(self.slot.read_oauth(), Ok(Some(c)) if &c == previous) {
            SwitchError::CredentialsFile
        } else {
            SwitchError::RollbackFailed
        }
    }

    /// Writes the target's `oauthAccount`; on failure puts `previous` back over the credential
    /// the switch wrote (never over a newer one a session refreshed since).
    fn write_config(&self, entry: &VaultEntry, previous: Option<&OAuthCredential>) -> Result<(), SwitchError> {
        if self.config.write_oauth_account(&entry.oauth_account).is_ok() {
            return Ok(());
        }
        let restored = previous.is_some_and(|p| self.slot.write_oauth(p, Some(&entry.oauth)).is_ok());
        Err(if restored { SwitchError::Config } else { SwitchError::RollbackFailed })
    }

    // ---- Helpers ----

    /// A session refreshing the active account during a switch would write its newest token
    /// after the app saved the account. Wait for that refresh to land; a token further past
    /// expiry has no session refreshing it, and on timeout the switch continues anyway.
    fn wait_for_imminent_refresh(&self) {
        let Ok(Some(expires)) = self.slot.read_oauth().map(|c| c.and_then(|c| c.expires_at_ms)) else { return };
        let now = (self.now_ms)();
        if expires < now - REFRESH_GRACE_MS || expires > now + REFRESH_LEAD_MS {
            return;
        }
        for _ in 0..MAX_POLLS {
            (self.sleep)(POLL_INTERVAL);
            let latest = self.slot.read_oauth().ok().flatten().and_then(|c| c.expires_at_ms);
            if latest != Some(expires) {
                return;
            }
        }
    }

    /// The dashboard account, other than `active_id`, whose vault copy has the same
    /// `refreshTokenExpiresAt` as `credential` (a fixed per-grant deadline), so `credential`
    /// is that account's login whatever `.claude.json` says.
    fn other_owner<'a>(
        &self,
        credential: &OAuthCredential,
        active_id: &str,
        accounts: &'a [SwitchAccount],
    ) -> Option<(&'a SwitchAccount, VaultEntry)> {
        let deadline = credential.refresh_expires_at_ms?;
        accounts.iter().filter(|a| a.id != active_id).find_map(|a| {
            let e = self.vault.load(&a.id)?;
            (e.oauth.refresh_expires_at_ms == Some(deadline)).then_some((a, e))
        })
    }

    fn is_expired(&self, e: &VaultEntry) -> bool {
        e.oauth.refresh_expires_at_ms.is_some_and(|d| d <= (self.now_ms)())
    }
}

/// Why `install_target` did not install: a refresh landed after the read (retry), or a
/// final error.
enum Install {
    Changed,
    Failed(SwitchError),
}

fn match_email<'a>(email: &str, accounts: &'a [SwitchAccount]) -> Option<&'a SwitchAccount> {
    accounts.iter().find(|a| a.email.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(email)))
}

fn log_mismatch(owner: &str, active: &str) {
    eprintln!("[cc-switch] capture skipped: credential belongs to {owner}, config names {active}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cc_switch::model::canonical;
    use serde_json::json;
    use std::sync::Arc;

    const NOW: i64 = 1_000_000_000_000;
    const DAY: i64 = 86_400_000;

    fn cred(rt: &str, expires: i64, deadline: i64) -> OAuthCredential {
        OAuthCredential::from_value(&json!({"accessToken": "a", "refreshToken": rt,
            "expiresAt": expires, "refreshTokenExpiresAt": deadline})).unwrap()
    }

    fn account_json(email: &str) -> String {
        canonical(&json!({"emailAddress": email, "accountUuid": format!("u-{email}")})).unwrap()
    }

    fn entry(rt: &str, email: &str, deadline: i64) -> VaultEntry {
        VaultEntry { oauth: cred(rt, NOW + DAY, deadline), oauth_account: account_json(email) }
    }

    #[derive(Default)]
    struct State {
        slot: Option<OAuthCredential>,
        config: Option<String>,
        vault: HashMap<String, VaultEntry>,
        /// Next N slot writes return ChangedSinceRead after a "session refresh" rotates the slot.
        refreshes_before_write: u32,
        /// Next N slot writes fail with Io (after landing when `write_lands`).
        failing_writes: u32,
        write_lands: bool,
        config_write_fails: bool,
        /// After this many reads the slot's expiresAt moves (a refresh landed).
        moves_after_reads: Option<u32>,
        reads: u32,
        sleeps: u32,
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<State>>);

    impl CredentialSlot for Fake {
        fn read_oauth(&self) -> Result<Option<OAuthCredential>, SlotError> {
            let mut s = self.0.lock().unwrap();
            s.reads += 1;
            if s.moves_after_reads.is_some_and(|n| s.reads > n) {
                if let Some(c) = s.slot.clone() {
                    s.slot = Some(cred(&c.refresh_token, NOW + DAY, c.refresh_expires_at_ms.unwrap_or(0)));
                }
            }
            Ok(s.slot.clone())
        }
        fn write_oauth(&self, c: &OAuthCredential, expecting: Option<&OAuthCredential>) -> Result<(), SlotError> {
            let mut s = self.0.lock().unwrap();
            if s.refreshes_before_write > 0 {
                s.refreshes_before_write -= 1;
                let cur = s.slot.clone().unwrap();
                s.slot = Some(cred(&format!("{}+", cur.refresh_token), NOW + DAY, cur.refresh_expires_at_ms.unwrap()));
                return Err(SlotError::ChangedSinceRead);
            }
            if s.slot.as_ref() != expecting {
                return Err(SlotError::ChangedSinceRead);
            }
            if s.failing_writes > 0 {
                s.failing_writes -= 1;
                if s.write_lands {
                    s.slot = Some(c.clone());
                }
                return Err(SlotError::Io("disk".into()));
            }
            s.slot = Some(c.clone());
            Ok(())
        }
    }

    impl ConfigFile for Fake {
        fn read_oauth_account(&self) -> Option<String> {
            self.0.lock().unwrap().config.clone()
        }
        fn write_oauth_account(&self, json: &str) -> Result<(), String> {
            let mut s = self.0.lock().unwrap();
            if s.config_write_fails {
                return Err("locked".into());
            }
            s.config = Some(json.to_string());
            Ok(())
        }
    }

    impl Vault for Fake {
        fn load(&self, id: &str) -> Option<VaultEntry> {
            self.0.lock().unwrap().vault.get(id).cloned()
        }
        fn save(&self, id: &str, e: &VaultEntry) -> Result<(), String> {
            self.0.lock().unwrap().vault.insert(id.into(), e.clone());
            Ok(())
        }
        fn delete(&self, id: &str) {
            self.0.lock().unwrap().vault.remove(id);
        }
    }

    fn switcher(f: &Fake) -> Switcher {
        let sleeps = f.clone();
        Switcher::new(
            Box::new(f.clone()),
            Box::new(f.clone()),
            Box::new(f.clone()),
            Box::new(|| NOW),
            Box::new(move |_| sleeps.0.lock().unwrap().sleeps += 1),
        )
    }

    fn accts() -> Vec<SwitchAccount> {
        vec![
            SwitchAccount { id: "A".into(), email: Some("a@x.com".into()) },
            SwitchAccount { id: "B".into(), email: Some("b@x.com".into()) },
        ]
    }

    /// A active with live credential "ra" (deadline NOW+20d); B captured with "rb" (NOW+25d).
    fn two_accounts() -> Fake {
        let f = Fake::default();
        {
            let mut s = f.0.lock().unwrap();
            s.slot = Some(cred("ra", NOW + DAY, NOW + 20 * DAY));
            s.config = Some(account_json("a@x.com"));
            s.vault.insert("B".into(), entry("rb", "b@x.com", NOW + 25 * DAY));
        }
        f
    }

    fn state(f: &Fake) -> std::sync::MutexGuard<'_, State> {
        f.0.lock().unwrap()
    }

    #[test]
    fn capture_saves_changed_and_skips_unchanged() {
        let f = two_accounts();
        let s = switcher(&f);
        assert_eq!(s.capture(&accts()), Ok(Capture::Saved("A".into())));
        assert_eq!(state(&f).vault["A"].oauth.refresh_token, "ra");
        assert_eq!(s.capture(&accts()), Ok(Capture::Unchanged("A".into())));
        state(&f).config = Some(account_json("nobody@x.com"));
        assert_eq!(s.capture(&accts()), Ok(Capture::NoActiveAccount));
    }

    #[test]
    fn capture_blank_is_blanked() {
        let f = two_accounts();
        state(&f).slot = Some(cred("", 0, NOW + 20 * DAY));
        assert_eq!(switcher(&f).capture(&accts()), Ok(Capture::Blanked("A".into())));
        assert!(!state(&f).vault.contains_key("A"));
    }

    #[test]
    fn capture_mismatch_not_saved() {
        let f = two_accounts();
        // The slot holds B's login (same fixed deadline as B's vault copy) while config names A.
        state(&f).slot = Some(cred("rb2", NOW + DAY, NOW + 25 * DAY));
        assert_eq!(switcher(&f).capture(&accts()), Ok(Capture::Mismatch("A".into())));
        assert!(!state(&f).vault.contains_key("A"));
    }

    #[test]
    fn availability_states() {
        let f = two_accounts();
        let mut all = accts();
        all.push(SwitchAccount { id: "C".into(), email: Some("c@x.com".into()) });
        all.push(SwitchAccount { id: "D".into(), email: Some("d@x.com".into()) });
        state(&f).vault.insert("D".into(), entry("rd", "d@x.com", NOW - 1));
        let m = switcher(&f).availability(&all);
        assert_eq!(m["A"], Availability::Active);
        assert_eq!(m["B"], Availability::Ready);
        assert_eq!(m["C"], Availability::NotCaptured);
        assert_eq!(m["D"], Availability::LoginExpired);
        state(&f).slot = Some(cred("", 0, 0));
        assert_eq!(switcher(&f).availability(&all)["A"], Availability::NeedsLogin);
        assert_eq!(Availability::Active.help(), None);
        assert_eq!(Availability::Ready.help(), Some("Use this account in Claude Code"));
        assert_eq!(Availability::NotCaptured.help(), Some("Sign this account in (opens its browser profile), then switch"));
        assert_eq!(Availability::LoginExpired.help(), Some("Login expired (~30 days); sign in again in the browser, then switch"));
        assert_eq!(Availability::NeedsLogin.help(), Some("Claude Code lost this login. Run /login with this account"));
    }

    #[test]
    fn switch_noop_when_target_active() {
        let f = two_accounts();
        assert_eq!(switcher(&f).switch_to(&accts()[0], &accts()), Ok(false));
        assert_eq!(state(&f).slot.as_ref().unwrap().refresh_token, "ra");
    }

    #[test]
    fn switch_refuses_not_captured_expired_and_other_email() {
        let f = two_accounts();
        let c = SwitchAccount { id: "C".into(), email: Some("c@x.com".into()) };
        assert_eq!(switcher(&f).switch_to(&c, &accts()), Err(SwitchError::NotCaptured));
        state(&f).vault.insert("C".into(), entry("rc", "someone@x.com", NOW + DAY));
        assert_eq!(switcher(&f).switch_to(&c, &accts()), Err(SwitchError::NotCaptured));
        state(&f).vault.insert("C".into(), entry("rc", "C@X.com", NOW - 1));
        assert_eq!(switcher(&f).switch_to(&c, &accts()), Err(SwitchError::LoginExpired));
        assert_eq!(state(&f).slot.as_ref().unwrap().refresh_token, "ra", "nothing changed");
    }

    #[test]
    fn switch_refuses_unknown_active_login() {
        let f = two_accounts();
        state(&f).config = Some(account_json("stranger@x.com"));
        assert_eq!(
            switcher(&f).switch_to(&accts()[1], &accts()),
            Err(SwitchError::ActiveAccountNotInDashboard("stranger@x.com".into()))
        );
        state(&f).config = None;
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Err(SwitchError::ActiveAccountUnknown));
        assert_eq!(state(&f).slot.as_ref().unwrap().refresh_token, "ra");
    }

    #[test]
    fn switch_saves_active_then_installs_target() {
        let f = two_accounts();
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Ok(true));
        let s = state(&f);
        assert_eq!(s.vault["A"].oauth.refresh_token, "ra", "A kept alive");
        assert_eq!(s.vault["A"].oauth_account, account_json("a@x.com"));
        assert_eq!(s.slot.as_ref().unwrap().refresh_token, "rb");
        assert_eq!(s.config.as_deref(), Some(account_json("b@x.com").as_str()));
    }

    #[test]
    fn changed_since_read_retries_then_succeeds() {
        let f = two_accounts();
        state(&f).refreshes_before_write = 1;
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Ok(true));
        assert_eq!(state(&f).vault["A"].oauth.refresh_token, "ra+", "the refreshed token was saved");
    }

    #[test]
    fn changed_three_times_is_credentials_file_error() {
        let f = two_accounts();
        state(&f).refreshes_before_write = 3;
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Err(SwitchError::CredentialsFile));
        assert_eq!(state(&f).config.as_deref(), Some(account_json("a@x.com").as_str()));
    }

    #[test]
    fn config_failure_rolls_back_credential() {
        let f = two_accounts();
        state(&f).config_write_fails = true;
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Err(SwitchError::Config));
        assert_eq!(state(&f).slot.as_ref().unwrap().refresh_token, "ra");
    }

    #[test]
    fn failed_write_is_undone() {
        let f = two_accounts();
        {
            let mut s = state(&f);
            s.failing_writes = 1;
            s.write_lands = true;
        }
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Err(SwitchError::CredentialsFile));
        assert_eq!(state(&f).slot.as_ref().unwrap().refresh_token, "ra", "restored");
    }

    #[test]
    fn waits_for_imminent_refresh() {
        let f = two_accounts();
        {
            let mut s = state(&f);
            s.slot = Some(cred("ra", NOW + 100_000, NOW + 20 * DAY));
            s.moves_after_reads = Some(2);
        }
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Ok(true));
        assert_eq!(state(&f).sleeps, 2);
    }

    #[test]
    fn no_wait_when_far_from_expiry() {
        let f = two_accounts();
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Ok(true));
        assert_eq!(state(&f).sleeps, 0);
        let f = two_accounts();
        state(&f).slot = Some(cred("ra", NOW - 120_000, NOW + 20 * DAY));
        assert_eq!(switcher(&f).switch_to(&accts()[1], &accts()), Ok(true));
        assert_eq!(state(&f).sleeps, 0, "long past expiry: no session is refreshing it");
    }

    #[test]
    fn email_match_is_case_insensitive() {
        let f = two_accounts();
        state(&f).config = Some(account_json("A@X.COM"));
        assert_eq!(switcher(&f).capture(&accts()), Ok(Capture::Saved("A".into())));
    }

    #[test]
    fn store_and_forget() {
        let f = two_accounts();
        let s = switcher(&f);
        s.store("C", &entry("rc", "c@x.com", NOW + DAY)).unwrap();
        assert!(state(&f).vault.contains_key("C"));
        s.forget("C");
        assert!(!state(&f).vault.contains_key("C"));
    }

    #[test]
    fn messages_match_macos() {
        let n = "b@x.com";
        assert_eq!(SwitchError::NotCaptured.message(n), "Run /login once in Claude Code with b@x.com so the dashboard can keep its login.");
        assert_eq!(SwitchError::LoginExpired.message(n), "The saved Claude Code login for b@x.com has expired. Run /login once with it.");
        assert_eq!(
            SwitchError::ActiveAccountNotInDashboard("s@x.com".into()).message(n),
            "Claude Code is signed in as s@x.com, which is not in the dashboard. Add it first, or its login would be lost."
        );
        assert_eq!(SwitchError::CredentialsFile.message(n), "Could not update Claude Code's credentials file. Nothing was changed.");
        assert_eq!(SwitchError::Config.message(n), "Could not update %USERPROFILE%\\.claude.json. The previous account was restored.");
        assert_eq!(SwitchError::VerifyFailed.message(n), "The switch could not be confirmed. Check /status in Claude Code.");
        assert!(SwitchError::ActiveAccountUnknown.message(n).starts_with("Claude Code is signed in to an account that %USERPROFILE%\\.claude.json does not name."));
        assert!(SwitchError::RollbackFailed.message(n).starts_with("Claude Code's credentials file may now hold b@x.com's login"));
        assert_eq!(SwitchError::RollbackFailed.log_name(), "rollbackFailed");
    }
}
