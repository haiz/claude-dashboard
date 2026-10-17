//! Add-account glue for pasted and scanned session keys. Plain blocking
//! functions: the UI calls them from a worker thread, never the UI thread.
//!
//! Paste mirrors the bridge handler: validate (`/api/account`) -> fetch orgs
//! -> lock -> load -> `apply_session_key` -> save. Scan mirrors macOS
//! `SetupView.scan`: each profile's key is checked with `/api/account` before
//! it is offered, accounts already stored are skipped with a reason, and the
//! picked ones are added as scanned (`source: browser`) accounts. The session
//! key never appears in an outcome or a message.

use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use claude_dashboard_core::api::{fetch_account, fetch_organizations, parse_account, ApiError};
use claude_dashboard_core::identity::{duplicate_index, resolve_org_id, StoredIdentity};
use claude_dashboard_core::key_intake::{apply_session_key, IntakeOutcome};
use claude_dashboard_core::manual_key::trimmed_key;
use claude_dashboard_core::model::{Account, AccountPlan, AccountSource, AccountStatus, Browser};
use claude_dashboard_core::plan::{detect_plan_tier, parse_orgs};
use claude_dashboard_core::scan::ProfileScanStatus;
use claude_dashboard_core::store::{self, encrypt_session_key};
use uuid::Uuid;

/// Reference-date (2001-01-01) offset from the Unix epoch, as `last_synced` uses.
const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0;

/// What applying a pasted key did (macOS `ManualKeyOutcome`, plus a store
/// failure macOS cannot report).
#[derive(Debug, Clone, PartialEq)]
pub enum AddOutcome {
    Added(String),
    Updated(String),
    /// Repaired, but the account has no organisation with chat access, so usage
    /// will not update. The key is still saved.
    UpdatedNoChatOrg(String),
    NoChatOrg,
    NotAccepted,
    EmptyKey,
    StoreError(String),
}

impl AddOutcome {
    /// The line the Paste tab shows; the strings of macOS `ManualKeyOutcome.message`.
    pub fn message(&self) -> String {
        match self {
            AddOutcome::Added(n) => format!("Added {n}."),
            AddOutcome::Updated(n) => format!("Updated the session key for {n}."),
            AddOutcome::UpdatedNoChatOrg(n) => format!(
                "Updated the session key for {n}, but that account has no organization with \
                 chat access, so usage will not update."
            ),
            AddOutcome::NoChatOrg => "That account has no organization with chat access.".to_string(),
            AddOutcome::NotAccepted => "That session key was not accepted. It may have expired, \
                 or been copied incompletely."
                .to_string(),
            AddOutcome::EmptyKey => "Paste a session key first.".to_string(),
            AddOutcome::StoreError(e) => format!("Could not save the account: {e}"),
        }
    }

    pub fn is_failure(&self) -> bool {
        !matches!(
            self,
            AddOutcome::Added(_) | AddOutcome::Updated(_) | AddOutcome::UpdatedNoChatOrg(_)
        )
    }

    /// True when the wizard must stay open after a success so the warning gets read.
    pub fn holds_open(&self) -> bool {
        matches!(self, AddOutcome::UpdatedNoChatOrg(_))
    }
}

/// Pure mapping from the shared intake outcome to the UI-facing one.
fn map_outcome(outcome: IntakeOutcome) -> AddOutcome {
    match outcome {
        IntakeOutcome::Added { name, .. } => AddOutcome::Added(name),
        IntakeOutcome::Updated {
            name,
            warn_no_chat_org: true,
            ..
        } => AddOutcome::UpdatedNoChatOrg(name),
        IntakeOutcome::Updated { name, .. } => AddOutcome::Updated(name),
        IntakeOutcome::RejectNoChatOrg => AddOutcome::NoChatOrg,
    }
}

fn now_reference() -> f64 {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    unix - REFERENCE_EPOCH_OFFSET
}

fn new_account_id() -> String {
    Uuid::new_v4().to_string().to_uppercase()
}

/// Validates a pasted key over the network, then adds or repairs the account
/// under the store lock. An unaccepted key writes nothing.
pub fn add_from_session_key(session_key: &str) -> AddOutcome {
    let Some(key) = trimmed_key(session_key) else {
        return AddOutcome::EmptyKey;
    };
    let Some(identity) = fetch_account(key).ok().and_then(|b| parse_account(&b)) else {
        return AddOutcome::NotAccepted;
    };
    let orgs = fetch_organizations(key)
        .ok()
        .map(|b| parse_orgs(&b))
        .unwrap_or_default();

    let _lock = match store::lock_store() {
        Ok(l) => l,
        Err(e) => return AddOutcome::StoreError(e.to_string()),
    };
    let mut accounts = match store::load_accounts_for_write() {
        Ok((a, _)) => a,
        Err(e) => return AddOutcome::StoreError(e.to_string()),
    };
    let outcome = apply_session_key(&mut accounts, key, &identity, &orgs, new_account_id, now_reference());
    if outcome == IntakeOutcome::RejectNoChatOrg {
        return AddOutcome::NoChatOrg;
    }
    if let Err(e) = store::save_accounts(&accounts) {
        return AddOutcome::StoreError(e.to_string());
    }
    map_outcome(outcome)
}

// ---- scan ----

/// One profile the scan offers to add (macOS `DetectedAccount`).
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedAccount {
    pub browser: Browser,
    pub org_id: String,
    pub account_uuid: String,
    pub profile_dir: String,
    pub profile_name: String,
    pub google_email: Option<String>,
    pub session_key: String,
    /// The email, else the profile's display name.
    pub name: String,
    pub email: Option<String>,
    pub plan: Option<AccountPlan>,
}

impl DetectedAccount {
    /// The row's second line: `"<Browser>: <Google email, else profile dir>"`.
    pub fn detail(&self) -> String {
        let place = match &self.google_email {
            Some(e) if !e.is_empty() => e.as_str(),
            _ => self.profile_dir.as_str(),
        };
        format!("{}: {place}", self.browser.display_name())
    }
}

/// Why one scanned profile was not offered (macOS `ScanOutcome`, plus the
/// Windows-only app-bound case).
#[derive(Debug, Clone, PartialEq)]
pub enum ScanOutcome {
    /// The session is valid and its account is already stored (dedupe is per
    /// account, not per profile; `contract/cases/dedupe.json`).
    AlreadyAdded { profile: String, account: String },
    /// `/api/account` rejected the key: the profile is signed out.
    SessionRejected { profile: String },
    /// Valid, but no organisation with chat access, so there is nothing to poll.
    NoChatOrg { profile: String },
    /// The call never completed, or its answer could not be read.
    Unreachable { profile: String },
    /// The profile's cookies are `v20` app-bound; only the extension can read it.
    AppBound { profile: String },
}

impl ScanOutcome {
    fn profile(&self) -> &str {
        match self {
            ScanOutcome::AlreadyAdded { profile, .. }
            | ScanOutcome::SessionRejected { profile }
            | ScanOutcome::NoChatOrg { profile }
            | ScanOutcome::Unreachable { profile }
            | ScanOutcome::AppBound { profile } => profile,
        }
    }

    /// The line shown for this profile, without the profile name.
    pub fn explanation(&self) -> String {
        match self {
            ScanOutcome::AlreadyAdded { account, .. } => format!("already added as {account}"),
            ScanOutcome::SessionRejected { .. } => {
                "signed out \u{2014} sign in to claude.ai in this profile".to_string()
            }
            ScanOutcome::NoChatOrg { .. } => "no organization with chat access".to_string(),
            ScanOutcome::Unreachable { .. } => "could not be read".to_string(),
            ScanOutcome::AppBound { .. } => {
                "protected by app-bound encryption \u{2014} use the Browser extension tab".to_string()
            }
        }
    }
}

/// Natural order, so "Profile 2" sorts before "Profile 10" (macOS
/// `localizedStandardCompare`). Case-insensitive.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn chunks(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.to_lowercase().chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    }
    let (ca, cb) = (chunks(a), chunks(b));
    for (x, y) in ca.iter().zip(&cb) {
        let o = match (x, y) {
            ((true, n), (true, m)) => {
                let (n, m) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                n.len().cmp(&m.len()).then_with(|| n.cmp(m))
            }
            _ => x.1.cmp(&y.1),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ca.len().cmp(&cb.len())
}

/// The message to show when the scan offers nothing, or `None` when it does
/// (macOS `ScanSummary.message`). `None` for an empty scan too: that sentence
/// is the caller's, see [`no_sessions_message`].
pub fn scan_summary(offered: usize, skipped: &[ScanOutcome]) -> Option<String> {
    if offered > 0 || skipped.is_empty() {
        return None;
    }
    let mut sorted: Vec<&ScanOutcome> = skipped.iter().collect();
    sorted.sort_by(|a, b| natural_cmp(a.profile(), b.profile()));
    let lines = sorted
        .into_iter()
        .map(|o| format!("\u{2022} {}: {}", o.profile(), o.explanation()));
    Some(
        std::iter::once("Nothing new to add. What the scan found:".to_string())
            .chain(lines)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The empty-scan sentence (SetupView.swift `scan()`).
pub fn no_sessions_message(browser: &Browser) -> String {
    let b = browser.display_name();
    format!(
        "No {b} profiles found with active Claude sessions. Make sure you're logged into \
         claude.ai in {b}."
    )
}

/// Shown when no supported browser is installed.
pub const NO_BROWSER_MESSAGE: &str = "No supported browser found (Chrome, Chrome Dev, Brave, Edge).";

/// The network calls of a scan, injected so tests touch no network.
pub trait ScanEnv {
    /// The raw `/api/account` body.
    fn fetch_account(&self, key: &str) -> Result<String, ApiError>;
    /// The raw `/api/organizations` body.
    fn fetch_orgs(&self, key: &str) -> Result<String, ApiError>;
}

pub struct RealScanEnv;

impl ScanEnv for RealScanEnv {
    fn fetch_account(&self, key: &str) -> Result<String, ApiError> {
        fetch_account(key)
    }
    fn fetch_orgs(&self, key: &str) -> Result<String, ApiError> {
        fetch_organizations(key)
    }
}

/// The identities dedupe compares against; empty on a read error.
pub fn stored_identities() -> Vec<StoredIdentity> {
    store::load_accounts()
        .map(|v| {
            v.into_iter()
                .map(|a| StoredIdentity {
                    account_uuid: a.account_uuid,
                    email: a.email,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Turns one browser's scanned profiles into the accounts to offer and the
/// reasons the rest were skipped (macOS `SetupView.scan`). Profiles without a
/// session are dropped silently. `None` when the browser had no session at
/// all, so the caller shows [`no_sessions_message`].
pub fn detect_accounts(
    profiles: Vec<(String, ProfileScanStatus)>,
    stored: &[StoredIdentity],
    env: &dyn ScanEnv,
) -> Option<(Vec<DetectedAccount>, Vec<ScanOutcome>)> {
    let mut offered = Vec::new();
    let mut skipped = Vec::new();
    let mut any = false;
    for (profile, status) in profiles {
        let s = match status {
            ProfileScanStatus::NoSession => continue,
            ProfileScanStatus::AppBound => {
                any = true;
                skipped.push(ScanOutcome::AppBound { profile });
                continue;
            }
            ProfileScanStatus::Found(s) => s,
        };
        any = true;
        // Identity from /api/account; a rejected key is a signed-out profile.
        let info = match env.fetch_account(&s.session_key) {
            Ok(body) => match parse_account(&body) {
                Some(i) => i,
                None => {
                    skipped.push(ScanOutcome::Unreachable { profile });
                    continue;
                }
            },
            Err(ApiError::HttpError(401 | 403)) => {
                skipped.push(ScanOutcome::SessionRejected { profile });
                continue;
            }
            Err(_) => {
                skipped.push(ScanOutcome::Unreachable { profile });
                continue;
            }
        };
        // The org to poll; never an identity (contract/cases/org-selection.json).
        let Some(org_id) = resolve_org_id(s.org_id.as_deref(), &info.memberships) else {
            skipped.push(ScanOutcome::NoChatOrg { profile });
            continue;
        };
        let name = info.email.clone().unwrap_or_else(|| s.display_name.clone());
        if duplicate_index(&info.uuid, info.email.as_deref(), stored).is_some() {
            skipped.push(ScanOutcome::AlreadyAdded {
                profile,
                account: name,
            });
            continue;
        }
        // Plan tier from the org's capabilities; unknown when the fetch fails.
        let plan = env
            .fetch_orgs(&s.session_key)
            .ok()
            .map(|b| parse_orgs(&b))
            .and_then(|orgs| {
                orgs.iter()
                    .find(|o| o.uuid == org_id)
                    .and_then(|o| detect_plan_tier(&o.raw, &o.capabilities))
            });
        offered.push(DetectedAccount {
            browser: s.browser,
            org_id,
            account_uuid: info.uuid,
            profile_dir: s.profile_dir,
            profile_name: s.display_name,
            google_email: s.google_email,
            session_key: s.session_key,
            name,
            email: info.email,
            plan,
        });
    }
    any.then_some((offered, skipped))
}

/// Appends `d` as a scanned account unless its identity is already stored
/// (the store may have changed since the scan). Returns whether it was added.
pub fn apply_detected(
    accounts: &mut Vec<Account>,
    d: &DetectedAccount,
    new_id: impl FnOnce() -> String,
    now_reference: f64,
) -> bool {
    let stored: Vec<StoredIdentity> = accounts
        .iter()
        .map(|a| StoredIdentity {
            account_uuid: a.account_uuid.clone(),
            email: a.email.clone(),
        })
        .collect();
    if duplicate_index(&d.account_uuid, d.email.as_deref(), &stored).is_some() {
        return false;
    }
    let profile_label = match &d.google_email {
        Some(e) if !e.is_empty() => e.clone(),
        _ => d.profile_name.clone(),
    };
    accounts.push(Account {
        id: new_id(),
        name: d.name.clone(),
        email: d.email.clone(),
        chrome_profile_path: d.profile_dir.clone(),
        chrome_profile_name: Some(profile_label),
        org_id: Some(d.org_id.clone()),
        account_uuid: Some(d.account_uuid.clone()),
        session_key: Some(encrypt_session_key(&d.session_key)),
        browser: d.browser.clone(),
        plan: d.plan.clone().unwrap_or(AccountPlan::Pro),
        last_synced: Some(now_reference),
        status: AccountStatus::Active,
        is_pinned: false,
        source: AccountSource::Browser,
    });
    true
}

/// Adds the picked accounts in one store write. `Err` is the line to show.
pub fn add_detected(selected: &[DetectedAccount]) -> Result<(), String> {
    let fail = |e: String| format!("Could not save the accounts: {e}");
    let _lock = store::lock_store().map_err(|e| fail(e.to_string()))?;
    let (mut accounts, _) = store::load_accounts_for_write().map_err(|e| fail(e.to_string()))?;
    let now = now_reference();
    for d in selected {
        apply_detected(&mut accounts, d, new_account_id, now);
    }
    store::save_accounts(&accounts).map_err(|e| fail(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::scan::ScannedSession;
    use std::collections::HashMap;

    #[test]
    fn map_outcome_maps_each_variant() {
        assert_eq!(
            map_outcome(IntakeOutcome::Added {
                account_id: "i".into(),
                name: "a@x.com".into(),
                plan: AccountPlan::Pro
            }),
            AddOutcome::Added("a@x.com".into())
        );
        let updated = |warn| IntakeOutcome::Updated {
            account_id: "i".into(),
            name: "b@x.com".into(),
            old_plan: AccountPlan::Pro,
            new_plan: AccountPlan::Pro,
            warn_no_chat_org: warn,
        };
        assert_eq!(map_outcome(updated(false)), AddOutcome::Updated("b@x.com".into()));
        assert_eq!(
            map_outcome(updated(true)),
            AddOutcome::UpdatedNoChatOrg("b@x.com".into())
        );
        assert_eq!(map_outcome(IntakeOutcome::RejectNoChatOrg), AddOutcome::NoChatOrg);
    }

    #[test]
    fn paste_messages_match_macos() {
        assert_eq!(AddOutcome::Added("a".into()).message(), "Added a.");
        assert_eq!(
            AddOutcome::Updated("a".into()).message(),
            "Updated the session key for a."
        );
        assert_eq!(
            AddOutcome::UpdatedNoChatOrg("a".into()).message(),
            "Updated the session key for a, but that account has no organization with chat \
             access, so usage will not update."
        );
        assert_eq!(
            AddOutcome::NotAccepted.message(),
            "That session key was not accepted. It may have expired, or been copied incompletely."
        );
        assert_eq!(AddOutcome::EmptyKey.message(), "Paste a session key first.");
        assert!(AddOutcome::UpdatedNoChatOrg("a".into()).holds_open());
        assert!(!AddOutcome::UpdatedNoChatOrg("a".into()).is_failure());
        assert!(!AddOutcome::Added("a".into()).holds_open());
        assert!(AddOutcome::NotAccepted.is_failure());
    }

    #[test]
    fn empty_key_is_rejected() {
        assert_eq!(add_from_session_key("   "), AddOutcome::EmptyKey);
    }

    #[test]
    fn paste_rejects_unaccepted_key_without_writing() {
        let _env = crate::testenv::lock();
        let dir = tempfile::tempdir().unwrap();
        // Closed port: the validation fetch fails, so the key is "not accepted".
        // Serialised with other env-mutating tests via testenv::lock.
        std::env::set_var("CLAUDE_DASHBOARD_API_BASE", "http://127.0.0.1:1");
        std::env::set_var("APPDATA", dir.path());
        std::env::set_var("LOCALAPPDATA", dir.path());

        let out = add_from_session_key("sk-bad");

        assert_eq!(out, AddOutcome::NotAccepted);
        assert!(
            !claude_dashboard_core::store::accounts_path().exists(),
            "a rejected key must not create accounts.json"
        );
    }

    /// Answers keyed by session key; a missing key is a network failure.
    struct Fake {
        accounts: HashMap<&'static str, Result<String, ApiError>>,
        orgs: String,
    }

    impl ScanEnv for Fake {
        fn fetch_account(&self, key: &str) -> Result<String, ApiError> {
            self.accounts
                .get(key)
                .cloned()
                .unwrap_or(Err(ApiError::Network("down".into())))
        }
        fn fetch_orgs(&self, _key: &str) -> Result<String, ApiError> {
            Ok(self.orgs.clone())
        }
    }

    fn account_body(uuid: &str, email: &str, caps: &str) -> Result<String, ApiError> {
        Ok(format!(
            r#"{{"uuid":"{uuid}","email_address":"{email}","memberships":[
                {{"organization":{{"uuid":"org-1","name":"O","capabilities":{caps}}}}}]}}"#
        ))
    }

    fn found(dir: &str, key: &str) -> (String, ProfileScanStatus) {
        (
            dir.to_string(),
            ProfileScanStatus::Found(ScannedSession {
                browser: Browser::Edge,
                profile_dir: dir.to_string(),
                display_name: format!("{dir} name"),
                google_email: Some(format!("{dir}@gmail.com")),
                session_key: key.to_string(),
                org_id: None,
            }),
        )
    }

    fn fake() -> Fake {
        let mut accounts = HashMap::new();
        accounts.insert("sk-new", account_body("u-new", "new@x.com", r#"["chat","claude_max"]"#));
        accounts.insert("sk-old", account_body("u-old", "old@x.com", r#"["chat"]"#));
        accounts.insert("sk-api", account_body("u-api", "api@x.com", r#"["api"]"#));
        accounts.insert("sk-out", Err(ApiError::HttpError(401)));
        accounts.insert("sk-junk", Ok("not json".to_string()));
        Fake {
            accounts,
            orgs: r#"[{"uuid":"org-1","name":"O","capabilities":["chat","claude_max"]}]"#.into(),
        }
    }

    #[test]
    fn scan_offers_new_accounts_and_explains_the_rest() {
        let stored = [StoredIdentity {
            account_uuid: Some("u-old".into()),
            email: None,
        }];
        let (offered, skipped) = detect_accounts(
            vec![
                found("Default", "sk-new"),
                found("Profile 1", "sk-old"),
                found("Profile 2", "sk-api"),
                found("Profile 3", "sk-out"),
                found("Profile 4", "sk-junk"),
                found("Profile 5", "sk-down"),
                ("Profile 6".into(), ProfileScanStatus::AppBound),
                ("Profile 7".into(), ProfileScanStatus::NoSession),
            ],
            &stored,
            &fake(),
        )
        .unwrap();

        assert_eq!(offered.len(), 1);
        let d = &offered[0];
        assert_eq!(d.name, "new@x.com");
        assert_eq!(d.account_uuid, "u-new");
        assert_eq!(d.org_id, "org-1");
        assert_eq!(d.plan, Some(AccountPlan::Max200));
        assert_eq!(d.detail(), "Microsoft Edge: Default@gmail.com");
        assert_eq!(
            skipped,
            vec![
                ScanOutcome::AlreadyAdded { profile: "Profile 1".into(), account: "old@x.com".into() },
                ScanOutcome::NoChatOrg { profile: "Profile 2".into() },
                ScanOutcome::SessionRejected { profile: "Profile 3".into() },
                ScanOutcome::Unreachable { profile: "Profile 4".into() },
                ScanOutcome::Unreachable { profile: "Profile 5".into() },
                ScanOutcome::AppBound { profile: "Profile 6".into() },
            ]
        );
    }

    #[test]
    fn scan_without_sessions_is_none() {
        let profiles = vec![("Default".to_string(), ProfileScanStatus::NoSession)];
        assert_eq!(detect_accounts(profiles, &[], &fake()), None);
        assert_eq!(detect_accounts(Vec::new(), &[], &fake()), None);
    }

    #[test]
    fn summary_lists_skipped_profiles_in_natural_order() {
        let skipped = [
            ScanOutcome::Unreachable { profile: "Profile 10".into() },
            ScanOutcome::AlreadyAdded { profile: "Profile 2".into(), account: "a@x.com".into() },
            ScanOutcome::SessionRejected { profile: "Default".into() },
        ];
        assert_eq!(scan_summary(1, &skipped), None);
        assert_eq!(scan_summary(0, &[]), None);
        assert_eq!(
            scan_summary(0, &skipped).unwrap(),
            "Nothing new to add. What the scan found:\n\
             \u{2022} Default: signed out \u{2014} sign in to claude.ai in this profile\n\
             \u{2022} Profile 2: already added as a@x.com\n\
             \u{2022} Profile 10: could not be read"
        );
    }

    #[test]
    fn empty_scan_message_names_the_browser() {
        assert_eq!(
            no_sessions_message(&Browser::Chrome),
            "No Google Chrome profiles found with active Claude sessions. Make sure you're \
             logged into claude.ai in Google Chrome."
        );
    }

    fn detected(uuid: &str, email: &str) -> DetectedAccount {
        DetectedAccount {
            browser: Browser::Brave,
            org_id: "org-1".into(),
            account_uuid: uuid.into(),
            profile_dir: "Profile 1".into(),
            profile_name: "Work".into(),
            google_email: None,
            session_key: "sk-x".into(),
            name: email.into(),
            email: Some(email.into()),
            plan: None,
        }
    }

    #[test]
    fn a_detected_account_is_stored_as_scanned() {
        let mut accounts = Vec::new();
        assert!(apply_detected(&mut accounts, &detected("u-1", "a@x.com"), || "ID".into(), 5.0));
        let a = &accounts[0];
        assert_eq!(a.id, "ID");
        assert_eq!(a.source, AccountSource::Browser);
        assert_eq!(a.browser, Browser::Brave);
        assert_eq!(a.chrome_profile_path, "Profile 1");
        assert_eq!(a.chrome_profile_name.as_deref(), Some("Work"));
        assert_eq!(a.account_uuid.as_deref(), Some("u-1"));
        assert_eq!(a.plan, AccountPlan::Pro);
        assert_eq!(a.status, AccountStatus::Active);
        assert_eq!(a.last_synced, Some(5.0));
        assert_eq!(
            store::decrypt_session_key(a.session_key.as_deref().unwrap()).as_deref(),
            Some("sk-x")
        );
    }

    #[test]
    fn a_detected_account_stored_since_the_scan_is_skipped() {
        let mut accounts = Vec::new();
        apply_detected(&mut accounts, &detected("u-1", "a@x.com"), || "A".into(), 0.0);
        assert!(!apply_detected(
            &mut accounts,
            &detected("u-1", "a@x.com"),
            || panic!("a duplicate must not mint an id"),
            0.0
        ));
        assert_eq!(accounts.len(), 1);
    }
}
