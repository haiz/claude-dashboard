//! `claude-dashboard-helper sync` — scans installed Chromium browsers for
//! Claude session cookies, identifies each candidate against
//! `GET /api/account`, and persists newly-found accounts.
//!
//! Mirrors `apps/macos/Helper/SyncCommand.swift`; see `contract/helper-cli.md`
//! "sync" for the shared, observable shape (the dedup key, the skip-not-fail
//! rule for expired sessions and unresolvable orgs, the `planHint ?? .pro`
//! default, and the exit codes). Browser discovery and the storage location
//! are platform detail (`core::browser`, `core::store`).
//!
//! Three things worth calling out, all matching the Swift source:
//! - **Identity is the Claude account's own uuid**, from `/api/account` —
//!   never the browser profile and never the org. `identity::is_duplicate`
//!   decides; see `contract/cases/dedupe.json`.
//! - The **orgId is chosen by rule**, not taken from the cookie: the
//!   `lastActiveOrg` cookie is only a preference, honoured when it names a
//!   chat org in the account's memberships. See
//!   `contract/cases/org-selection.json`. `/api/organizations` is still the
//!   source of the plan tier, and nothing else.
//! - A `v12` (secret-portal) cookie makes the whole profile unreadable by
//!   this port, so such a profile is silently skipped (out of scope, per
//!   Spike 0).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use claude_dashboard_core::api::{fetch_account, fetch_organizations, parse_account};
use claude_dashboard_core::browser::{self, DiscoveredProfile};
use claude_dashboard_core::cookie::{self, CookieError, PasswordSource};
use claude_dashboard_core::identity::{is_duplicate, resolve_org_id, StoredIdentity};
use claude_dashboard_core::model::{Account, AccountPlan, AccountStatus, Browser};
use claude_dashboard_core::plan::detect_plan_tier;
use claude_dashboard_core::store;
use serde_json::Value;
use uuid::Uuid;

/// 2001-01-01 -> 1970-01-01 offset, so `lastSynced` round-trips to the
/// macOS `Date` reference-date encoding (see `crate::model::Account`).
const REFERENCE_EPOCH_OFFSET: f64 = 978_307_200.0;

/// A browser profile that yielded a decrypted `sessionKey` — the shortlist
/// `run_sync` validates and may persist.
struct Candidate {
    browser: Browser,
    /// The account's `chromeProfilePath` (the profile directory name).
    profile_dir: String,
    /// Resolved profile display name (used in the pre-validation skip
    /// messages and as the account name when no e-mail is found).
    display_name: String,
    /// `chromeProfileName`: the Google e-mail if present, else the display
    /// name (mirrors the Swift `chromeLabel`).
    chrome_label: String,
    session_key: String,
    /// The `lastActiveOrg` cookie value, if it decrypted. Only a preference:
    /// [`resolve_org_id`] falls back to the first chat org when it is `None`
    /// or names an org the account cannot chat in.
    org_id: Option<String>,
}

/// Outcome of decrypting one profile's Claude cookies.
enum ProfileScan {
    /// At least one cookie was a `v12` blob and no portal secret is
    /// available — skip the whole profile.
    NoPortalSecret,
    /// No `sessionKey` cookie decrypted — not a Claude session.
    NoSession,
    /// A `sessionKey` was found (with the `lastActiveOrg`-derived org id, if
    /// any).
    Found {
        session_key: String,
        org_id: Option<String>,
    },
}

/// Runs the `sync` subcommand and returns its process exit code.
pub fn run_sync() -> i32 {
    eprintln!("Scanning installed browsers for Claude sessions...");

    let profiles = browser::discover_profiles_under(&home_dir());

    // Fetch each browser's Safe Storage secret at most once.
    let mut secrets: HashMap<String, Option<String>> = HashMap::new();
    let mut candidates: Vec<Candidate> = Vec::new();

    for profile in &profiles {
        let secret = secrets
            .entry(profile.keyring_app.clone())
            .or_insert_with(|| cookie::keyring_password(&profile.keyring_app));
        let source = match secret {
            Some(s) => PasswordSource::Keyring(s.clone()),
            None => PasswordSource::HardcodedV10,
        };

        match scan_profile(profile, &source) {
            ProfileScan::Found {
                session_key,
                org_id,
            } => candidates.push(Candidate {
                browser: profile.browser.clone(),
                profile_dir: profile.profile_dir.clone(),
                display_name: profile.resolved_display_name(),
                chrome_label: profile
                    .google_email_nonempty()
                    .unwrap_or_else(|| profile.resolved_display_name()),
                session_key,
                org_id,
            }),
            ProfileScan::NoPortalSecret | ProfileScan::NoSession => continue,
        }
    }

    if candidates.is_empty() {
        eprintln!("No browser profiles found with active Claude sessions.");
        eprintln!("Make sure you're logged into claude.ai in a supported browser.");
        return 1;
    }

    eprintln!(
        "Found {} profile(s) with Claude sessions. Validating...",
        candidates.len()
    );

    let mut accounts = store::load_accounts().unwrap_or_default();
    let mut added = 0usize;

    for c in &candidates {
        // Identity from /api/account. A failure is an unusable session.
        let info = match fetch_account(&c.session_key) {
            Ok(body) => match parse_account(&body) {
                Some(a) => a,
                None => {
                    eprintln!("  Skipping {} (session expired)", c.display_name);
                    continue;
                }
            },
            Err(_) => {
                eprintln!("  Skipping {} (session expired)", c.display_name);
                continue;
            }
        };

        // Same dedupe rule as the app: the Claude account, not the browser
        // profile. See contract/cases/dedupe.json.
        let stored: Vec<StoredIdentity> = accounts
            .iter()
            .map(|a| StoredIdentity {
                account_uuid: a.account_uuid.clone(),
                email: a.email.clone(),
            })
            .collect();
        if is_duplicate(&info.uuid, info.email.as_deref(), &stored) {
            eprintln!("  Skipping {} (already added)", c.display_name);
            continue;
        }

        let Some(org_id) = resolve_org_id(c.org_id.as_deref(), &info.memberships) else {
            eprintln!("  Skipping {} (no usable org)", c.display_name);
            continue;
        };

        let orgs = match fetch_organizations(&c.session_key) {
            Ok(body) => parse_orgs(&body),
            Err(_) => Vec::new(),
        };
        let plan = plan_for(&orgs, &org_id);
        let email = info.email.clone();
        let display_name = email.clone().unwrap_or_else(|| c.display_name.clone());
        let plan_wire = plan_wire_value(&plan);

        accounts.push(Account {
            id: new_account_id(),
            name: display_name.clone(),
            email,
            chrome_profile_path: c.profile_dir.clone(),
            chrome_profile_name: Some(c.chrome_label.clone()),
            org_id: Some(org_id),
            account_uuid: Some(info.uuid.clone()),
            session_key: Some(store::encrypt_session_key(&c.session_key)),
            browser: c.browser.clone(),
            plan,
            last_synced: Some(now_reference_seconds()),
            status: AccountStatus::Active,
            is_pinned: false,
        });
        added += 1;
        eprintln!("  Added: {display_name} ({plan_wire})");
    }

    if let Err(e) = store::save_accounts(&accounts) {
        // The Swift `saveAccounts` returns void and cannot report failure;
        // to preserve the exit-0-after-scan contract we log and still
        // return 0 rather than inventing a new failure mode.
        eprintln!("Warning: failed to save accounts: {e}");
    }

    if added == 0 {
        eprintln!("No new accounts to add (all already synced).");
    } else {
        eprintln!("Synced {added} account(s) successfully.");
    }
    0
}

/// Decrypts a profile's Claude cookies into a [`ProfileScan`]. A `v12`
/// cookie short-circuits the whole profile; any other per-cookie decrypt
/// error skips just that cookie (matching the Swift `guard let decrypted`
/// `continue`).
fn scan_profile(profile: &DiscoveredProfile, source: &PasswordSource) -> ProfileScan {
    let Some((schema, rows)) = browser::read_claude_cookie_db(&profile.cookies_db) else {
        return ProfileScan::NoSession;
    };
    let mut session_key = None;
    let mut org_id = None;
    for row in rows {
        match cookie::decrypt_cookie_value(&row.encrypted_value, &row.host_key, schema, source) {
            Ok(value) => match row.name.as_str() {
                "sessionKey" => session_key = Some(value),
                "lastActiveOrg" => org_id = Some(value),
                _ => {}
            },
            Err(CookieError::NoPortalSecret) => return ProfileScan::NoPortalSecret,
            Err(_) => continue,
        }
    }
    match session_key {
        Some(session_key) => ProfileScan::Found {
            session_key,
            org_id,
        },
        None => ProfileScan::NoSession,
    }
}

/// One parsed `/api/organizations` entry (only orgs carrying both `uuid`
/// and `name` survive, matching the Swift `compactMap`). `sync` reads this
/// solely for the plan tier — e-mail comes from `/api/account`.
struct ParsedOrg {
    uuid: String,
    capabilities: Vec<String>,
    /// The org's full JSON, handed to [`detect_plan_tier`] (steps 1-2 there
    /// scan the whole object, not just `capabilities`).
    raw: Value,
}

/// Parses the raw `/api/organizations` body into the orgs `sync` cares
/// about. Returns an empty vec when the body is not a JSON array, is empty,
/// or contains no org with both `uuid` and `name`.
fn parse_orgs(orgs_json: &str) -> Vec<ParsedOrg> {
    let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(orgs_json) else {
        return Vec::new();
    };
    arr.into_iter()
        .filter_map(|v| {
            let uuid = v.get("uuid")?.as_str()?.to_string();
            // Presence check only: a name-less org is filtered out, matching
            // the Swift `compactMap`. The name itself is never inspected.
            v.get("name")?.as_str()?;
            let capabilities = v
                .get("capabilities")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Some(ParsedOrg {
                uuid,
                capabilities,
                raw: v,
            })
        })
        .collect()
}

/// Plan tier for the chosen org. Unchanged behaviour: `detect_plan_tier` on
/// that org's raw JSON, defaulting to Pro when the org is absent or yields
/// nothing. The e-mail half of the old `email_and_plan` is gone — e-mail now
/// comes from `/api/account`, not from parsing an org name.
fn plan_for(orgs: &[ParsedOrg], org_id: &str) -> AccountPlan {
    orgs.iter()
        .find(|o| o.uuid == org_id)
        .and_then(|o| detect_plan_tier(&o.raw, &o.capabilities))
        .unwrap_or(AccountPlan::Pro)
}

/// The plan's on-the-wire string (`"Pro"`, `"Max 5x"`, `"Max 20x"`,
/// `"Max"`) — what the Swift `plan.rawValue` prints in the "Added:" line.
fn plan_wire_value(plan: &AccountPlan) -> String {
    match serde_json::to_value(plan) {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}

/// A fresh uppercase, hyphenated UUID — the account-id wire form
/// (`crate::model` round-trips it verbatim).
fn new_account_id() -> String {
    Uuid::new_v4().to_string().to_uppercase()
}

/// `$HOME` (or `.` when unset), matching `core::store`'s own home
/// resolution.
fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// The current time in Foundation reference-date seconds, so `lastSynced`
/// round-trips to the macOS `Date` encoding.
fn now_reference_seconds() -> f64 {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    unix - REFERENCE_EPOCH_OFFSET
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_orgs ---------------------------------------------------------

    #[test]
    fn parse_orgs_empty_for_non_array_or_empty_or_uuidless() {
        assert!(parse_orgs("[]").is_empty());
        assert!(parse_orgs(r#"{"not":"an array"}"#).is_empty());
        assert!(parse_orgs("not json at all").is_empty());
        // orgs missing uuid OR name are filtered (compactMap parity).
        assert!(parse_orgs(r#"[{"name":"no uuid"}]"#).is_empty());
        assert!(parse_orgs(r#"[{"uuid":"o1"}]"#).is_empty());
    }

    #[test]
    fn parse_orgs_reads_uuid_and_capabilities() {
        let json = r#"[
            {"uuid":"o1","name":"Team","capabilities":["chat"]},
            {"uuid":"o2","name":"Other"}
        ]"#;
        let orgs = parse_orgs(json);
        assert_eq!(orgs.len(), 2);
        assert_eq!(orgs[0].uuid, "o1");
        assert_eq!(orgs[0].capabilities, vec!["chat".to_string()]);
        assert!(orgs[1].capabilities.is_empty()); // absent -> empty, not an error
    }

    // -- plan_for -----------------------------------------------------------

    #[test]
    fn plan_from_matching_org_capability() {
        // claude_pro on the matching org -> Pro.
        let json = r#"[{"uuid":"o1","name":"Personal","capabilities":["claude_pro"]}]"#;
        assert_eq!(plan_for(&parse_orgs(json), "o1"), AccountPlan::Pro);
    }

    #[test]
    fn plan_max_capability_resolves_to_generic_max() {
        // claude_max with no 5x/20x marker -> the generic Max fallback.
        let json = r#"[{"uuid":"o1","name":"Team Alpha","capabilities":["claude_max"]}]"#;
        assert_eq!(plan_for(&parse_orgs(json), "o1"), AccountPlan::Max200);
    }

    #[test]
    fn plan_from_matching_org_marker() {
        // max_20x appears in the matching org's JSON (its name) -> Max20x.
        let json = r#"[{"uuid":"o1","name":"Acme max_20x","capabilities":["chat"]}]"#;
        assert_eq!(plan_for(&parse_orgs(json), "o1"), AccountPlan::Max20x);
    }

    #[test]
    fn plan_defaults_to_pro_when_org_id_matches_nothing() {
        // An org with a max_20x marker exists, but the resolved orgId
        // matches no org, so the plan is the Pro fallback, NOT Max20x.
        let json = r#"[{"uuid":"o1","name":"Acme max_20x","capabilities":["chat"]}]"#;
        assert_eq!(plan_for(&parse_orgs(json), "DOES-NOT-MATCH"), AccountPlan::Pro);
    }

    #[test]
    fn plan_defaults_to_pro_when_matching_org_has_no_tier() {
        // Matching org has no plan marker and no plan capability ->
        // detect_plan_tier returns None -> Pro fallback.
        let json = r#"[{"uuid":"o1","name":"API Only","capabilities":["api"]}]"#;
        assert_eq!(plan_for(&parse_orgs(json), "o1"), AccountPlan::Pro);
    }

    // -- plan_wire_value ----------------------------------------------------

    #[test]
    fn plan_wire_values_match_rawvalue() {
        assert_eq!(plan_wire_value(&AccountPlan::Pro), "Pro");
        assert_eq!(plan_wire_value(&AccountPlan::Max5x), "Max 5x");
        assert_eq!(plan_wire_value(&AccountPlan::Max20x), "Max 20x");
        assert_eq!(plan_wire_value(&AccountPlan::Max200), "Max");
    }

    // -- new_account_id -----------------------------------------------------

    #[test]
    fn account_id_is_uppercase_hyphenated_uuid() {
        let id = new_account_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_eq!(id, id.to_uppercase());
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    }
}
