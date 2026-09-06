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
//! - A duplicate is skipped for *adding* only: its stored plan tier is still
//!   refreshed, so a tier that fell back to `Pro` because
//!   `/api/organizations` was down at add time heals on the next `sync`. See
//!   `contract/README.md`'s "Refreshing a stored plan".
//! - A `v12` (secret-portal) cookie needs a secret this scan does not have
//!   up front. Such a profile is re-scanned once per candidate `app_id` with
//!   the portal secret fetched from the keyring; only if none of them decrypt
//!   is the profile skipped. The fetch is **lazy** — a profile with no v12
//!   cookie never touches the portal schema, so users on v10/v11 browsers
//!   cannot be prompted to unlock a keyring they did not need.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use claude_dashboard_core::api::{fetch_account, fetch_organizations, parse_account};
use claude_dashboard_core::browser::{self, DiscoveredProfile};
use claude_dashboard_core::cookie::{self, CookieError, KeySources};
use claude_dashboard_core::identity::{duplicate_index, resolve_org_id, StoredIdentity};
use claude_dashboard_core::model::{Account, AccountPlan, AccountSource, AccountStatus, Browser};
use claude_dashboard_core::plan::{detect_plan_tier, refreshed_plan};
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

    // Fetch each browser's Safe Storage secret at most once, and each
    // portal secret at most once per app_id.
    let mut secrets: HashMap<String, Option<String>> = HashMap::new();
    let mut portal_secrets: HashMap<String, Option<Vec<u8>>> = HashMap::new();
    let mut candidates: Vec<Candidate> = Vec::new();

    for profile in &profiles {
        let secret = secrets
            .entry(profile.keyring_app.clone())
            .or_insert_with(|| cookie::keyring_password(&profile.keyring_app))
            .clone();
        let sources = KeySources::keyring(secret);
        let app_ids = portal_app_id_candidates(profile);

        let scanned = scan_resolving_portal(
            &sources,
            &app_ids,
            |app_id| {
                portal_secrets
                    .entry(app_id.to_string())
                    .or_insert_with(|| cookie::portal_secret(app_id))
                    .clone()
            },
            |sources| scan_profile(profile, sources),
        );

        match scanned {
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
            ProfileScan::NoPortalSecret => {
                // Linux-only diagnostic: the profile is portal-encrypted and
                // no candidate app_id yielded a secret that decrypts it.
                // Silence here would look identical to "not logged in".
                eprintln!(
                    "  Skipping {} (portal-encrypted cookies, no usable secret)",
                    profile.resolved_display_name()
                );
                continue;
            }
            ProfileScan::NoSession => continue,
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
        if let Some(index) = duplicate_index(&info.uuid, info.email.as_deref(), &stored) {
            eprintln!("  Skipping {} (already added)", c.display_name);
            refresh_stored_plan(&mut accounts[index], &c.session_key, &c.display_name);
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
            source: AccountSource::Browser,
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

/// The `app_id` values to try when a profile turns out to hold `v12`
/// (secret-portal) cookies, in the order they are tried.
///
/// `app_id` is the *portal's* name for the caller. The xdg-desktop-portal
/// frontend assigns it; the browser never sends one
/// (`secret_portal_key_provider.cc` passes only the write fd and an options
/// dict). How the frontend derives it depends on how the browser was
/// installed, not on the browser itself:
///
/// - `""` — a host (unsandboxed) browser the frontend could not identify.
///   Real Chromium produced this in Spike 1, and a `.deb` install gets it on
///   a systemd desktop too: `xdp-app-info-host.c` reads the reverse-DNS id
///   Chromium puts in its transient scope (`app-com.google.Chrome-<pid>.scope`,
///   from `version_info::nix::GetAppName`) but then requires a matching
///   `com.google.Chrome.desktop`, which the `.deb` does not ship.
/// - the Flathub id ([`browser::flathub_id`]) — a Flatpak install, where the
///   frontend reads `/.flatpak-info`. For Chrome it equals the scope id
///   above, so it also covers a host install that does carry that desktop
///   file.
/// - `snap.<name>` ([`browser::snap_name`]) — a strict snap, which the
///   frontend identifies from the caller's cgroup (`xdp-app-info-snap.c`).
///   Only Brave has an official snap. Its `password-manager-service` plug is
///   not auto-connected, so out of the box the snap cannot reach the secret
///   service at all and writes `v10`; this id matters only once a user has
///   run `snap connect` and enabled the portal-encryption feature.
///
/// The libsecret `application` name (`"chrome"`, `"brave"`, ...) is
/// deliberately *not* a candidate: no code path in Chromium or the portal
/// produces it as an `app_id`. Not covered, and recorded as such in the
/// spike doc: Brave/Edge scope ids (fork-defined, unverified), and none of
/// the Flatpak/Snap ids has been observed against a real sandboxed install.
///
/// Trying several is safe: a wrong secret fails AES-256-GCM authentication,
/// so it can never yield a wrong plaintext.
fn portal_app_id_candidates(profile: &DiscoveredProfile) -> Vec<String> {
    let mut out = vec![String::new()];
    out.extend(browser::flathub_id(&profile.browser).map(String::from));
    out.extend(browser::snap_name(&profile.browser).map(|name| format!("snap.{name}")));
    out
}

/// Scans a profile, resolving a `v12` portal secret only if the first pass
/// reports one is needed.
///
/// `scan` is the profile scan and `fetch` the portal-secret lookup, both
/// injected so this ordering is testable without a keyring or a cookie DB.
/// Returns [`ProfileScan::NoPortalSecret`] when the profile needs a portal
/// secret that no candidate provided — a wrong secret leaves the `sessionKey`
/// cookie undecryptable, which the scan reports as
/// [`ProfileScan::NoSession`], so that outcome moves on to the next
/// candidate rather than ending the search.
fn scan_resolving_portal<F, S>(
    sources: &KeySources,
    app_ids: &[String],
    mut fetch: F,
    mut scan: S,
) -> ProfileScan
where
    F: FnMut(&str) -> Option<Vec<u8>>,
    S: FnMut(&KeySources) -> ProfileScan,
{
    match scan(sources) {
        ProfileScan::NoPortalSecret => {}
        settled => return settled,
    }
    for app_id in app_ids {
        let Some(secret) = fetch(app_id) else {
            continue;
        };
        match scan(&sources.with_portal(secret)) {
            ProfileScan::Found {
                session_key,
                org_id,
            } => {
                return ProfileScan::Found {
                    session_key,
                    org_id,
                }
            }
            _ => continue,
        }
    }
    ProfileScan::NoPortalSecret
}

/// Corrects the stored plan tier of an account this run skipped as a
/// duplicate — the CLI's counterpart to the GUI's per-refresh plan update
/// (`DashboardViewModel.refreshAll`). Mirrors the same block in
/// `apps/macos/Helper/SyncCommand.swift`; `contract/helper-cli.md` "sync"
/// specifies the extra stderr line.
///
/// Only `plan` is written — not `session_key`, not `last_synced`, not
/// `status`. An account with no `org_id` has no org to match against and is
/// left alone.
fn refresh_stored_plan(account: &mut Account, session_key: &str, display_name: &str) {
    // A failed fetch is `&[]`, which `refreshed_plan_for` turns into "no
    // write" — the same outcome as an org that carries no displayable plan.
    //
    // This `fetch_organizations` call is the one step of the heal no test
    // drives (`contract/helper-cli.md`, "Test coverage of the network
    // layer"); everything the outcome depends on lives in
    // [`apply_refreshed_plan`], which is tested.
    let orgs = match fetch_organizations(session_key) {
        Ok(body) => parse_orgs(&body),
        Err(_) => Vec::new(),
    };

    apply_refreshed_plan(account, &orgs, display_name, &mut io::stderr());
}

/// Writes the refreshed tier onto `account` and reports it on `out`, or
/// leaves the account untouched and writes nothing when [`refreshed_plan_for`]
/// says there is nothing to write. Split out of [`refresh_stored_plan`] so
/// both are reachable from tests without a network call: an empty `orgs` is
/// exactly what a failed fetch produces, and `out` is `stderr` in production.
///
/// The reported line is contract (`contract/helper-cli.md`, "sync"), which is
/// why it is written through `out` rather than `eprintln!`.
fn apply_refreshed_plan(
    account: &mut Account,
    orgs: &[ParsedOrg],
    display_name: &str,
    out: &mut impl Write,
) {
    if let Some(plan) = refreshed_plan_for(account, orgs) {
        // A helper that cannot report is still a helper that must heal: the
        // write below matters more than the line, so a failed write is
        // dropped rather than propagated (`eprintln!` would panic here).
        let _ = writeln!(
            out,
            "  Updated plan: {display_name} ({} -> {})",
            plan_wire_value(&account.plan),
            plan_wire_value(&plan)
        );
        account.plan = plan;
    }
}

/// The plan to persist for `account` given a freshly fetched
/// `/api/organizations` result — `None` to leave the stored plan alone.
///
/// Mirrors `UsageAPIService.refreshedPlan(for:orgs:)`. The org is matched on
/// the account's **stored** `org_id`: an account with no `org_id` is not
/// pollable and is never touched, and an `orgs` slice with no matching entry
/// (an empty one included, which is what a failed fetch produces) reduces to
/// rule 1 of [`refreshed_plan`].
///
/// Deliberately no `unwrap_or(Pro)` here: unlike the add path
/// ([`plan_for`]), an unresolved tier must leave the stored one as it is.
pub(crate) fn refreshed_plan_for(account: &Account, orgs: &[ParsedOrg]) -> Option<AccountPlan> {
    let org_id = account.org_id.as_deref()?;
    let hint = orgs
        .iter()
        .find(|o| o.uuid == org_id)
        .and_then(|o| detect_plan_tier(&o.raw, &o.capabilities));
    refreshed_plan(&account.plan, hint)
}

/// Decrypts a profile's Claude cookies into a [`ProfileScan`]. A `v12`
/// cookie short-circuits the whole profile; any other per-cookie decrypt
/// error skips just that cookie (matching the Swift `guard let decrypted`
/// `continue`).
fn scan_profile(profile: &DiscoveredProfile, sources: &KeySources) -> ProfileScan {
    let Some((schema, rows)) = browser::read_claude_cookie_db(&profile.cookies_db) else {
        return ProfileScan::NoSession;
    };
    let mut session_key = None;
    let mut org_id = None;
    for row in rows {
        match cookie::decrypt_cookie_value(&row.encrypted_value, &row.host_key, schema, sources) {
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
pub(crate) struct ParsedOrg {
    pub(crate) uuid: String,
    pub(crate) capabilities: Vec<String>,
    /// The org's full JSON, handed to [`detect_plan_tier`] (steps 1-2 there
    /// scan the whole object, not just `capabilities`).
    pub(crate) raw: Value,
}

/// Parses the raw `/api/organizations` body into the orgs `sync` cares
/// about. Returns an empty vec when the body is not a JSON array, is empty,
/// or contains no org with both `uuid` and `name`.
pub(crate) fn parse_orgs(orgs_json: &str) -> Vec<ParsedOrg> {
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
pub(crate) fn plan_for(orgs: &[ParsedOrg], org_id: &str) -> AccountPlan {
    orgs.iter()
        .find(|o| o.uuid == org_id)
        .and_then(|o| detect_plan_tier(&o.raw, &o.capabilities))
        .unwrap_or(AccountPlan::Pro)
}

/// The plan's on-the-wire string (`"Pro"`, `"Max 5x"`, `"Max 20x"`,
/// `"Max"`) — what the Swift `plan.rawValue` prints in the "Added:" line.
pub(crate) fn plan_wire_value(plan: &AccountPlan) -> String {
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
    use std::cell::RefCell;
    use std::path::PathBuf;

    // -- refreshed_plan_for ------------------------------------------------

    fn stored_account(org_id: Option<&str>, plan: AccountPlan) -> Account {
        Account {
            id: "ID".into(),
            name: "Test".into(),
            email: None,
            chrome_profile_path: "Default".into(),
            chrome_profile_name: None,
            org_id: org_id.map(String::from),
            account_uuid: Some("acct-1".into()),
            session_key: None,
            browser: Browser::Chrome,
            plan,
            last_synced: None,
            status: AccountStatus::Active,
            is_pinned: false,
            source: AccountSource::Browser,
        }
    }

    #[test]
    fn refreshed_plan_for_takes_the_hint_of_the_stored_org() {
        let orgs = parse_orgs(
            r#"[{"uuid":"org-2","name":"Other","capabilities":["chat","claude_pro"]},
                {"uuid":"org-1","name":"Personal","capabilities":["chat","claude_max"]}]"#,
        );
        let account = stored_account(Some("org-1"), AccountPlan::Pro);
        assert_eq!(refreshed_plan_for(&account, &orgs), Some(AccountPlan::Max200));
    }

    #[test]
    fn refreshed_plan_for_is_none_when_the_fetch_produced_nothing() {
        let account = stored_account(Some("org-1"), AccountPlan::Pro);
        assert_eq!(refreshed_plan_for(&account, &[]), None);
    }

    #[test]
    fn refreshed_plan_for_ignores_an_org_the_account_is_not_polled_against() {
        let orgs = parse_orgs(r#"[{"uuid":"org-2","name":"Other","capabilities":["chat"]}]"#);
        let account = stored_account(Some("org-1"), AccountPlan::Pro);
        assert_eq!(refreshed_plan_for(&account, &orgs), None);
    }

    #[test]
    fn refreshed_plan_for_is_none_without_an_org_id() {
        let orgs = parse_orgs(r#"[{"uuid":"org-1","name":"Personal","capabilities":["chat"]}]"#);
        let account = stored_account(None, AccountPlan::Pro);
        assert_eq!(refreshed_plan_for(&account, &orgs), None);
    }

    #[test]
    fn refreshed_plan_for_is_none_when_the_hint_matches_what_is_stored() {
        let orgs = parse_orgs(
            r#"[{"uuid":"org-1","name":"Personal","capabilities":["chat","claude_pro"]}]"#,
        );
        let account = stored_account(Some("org-1"), AccountPlan::Pro);
        assert_eq!(refreshed_plan_for(&account, &orgs), None);
    }

    // -- apply_refreshed_plan ----------------------------------------------

    #[test]
    fn apply_refreshed_plan_writes_the_refreshed_tier_onto_the_account() {
        let orgs = parse_orgs(
            r#"[{"uuid":"org-1","name":"Personal","capabilities":["chat","claude_max"]}]"#,
        );
        let mut account = stored_account(Some("org-1"), AccountPlan::Pro);

        let mut out = Vec::new();
        apply_refreshed_plan(&mut account, &orgs, "Default", &mut out);

        assert_eq!(account.plan, AccountPlan::Max200);
    }

    /// The `Err(_) => Vec::new()` arm of `refresh_stored_plan` reaches this
    /// function as an empty slice: a failed `/api/organizations` must leave
    /// the stored tier exactly as it was.
    #[test]
    fn apply_refreshed_plan_leaves_the_stored_tier_alone_when_the_fetch_failed() {
        let mut account = stored_account(Some("org-1"), AccountPlan::Max20x);

        let mut out = Vec::new();
        apply_refreshed_plan(&mut account, &[], "Default", &mut out);

        assert_eq!(account.plan, AccountPlan::Max20x);
        assert!(out.is_empty(), "a run that writes nothing reports nothing");
    }

    /// `contract/helper-cli.md` "sync": a write prints exactly this one extra
    /// line. Nothing else in the repo pins its shape.
    #[test]
    fn apply_refreshed_plan_reports_the_write_in_the_contract_line_shape() {
        let orgs = parse_orgs(
            r#"[{"uuid":"org-1","name":"Personal","capabilities":["chat","claude_max"]}]"#,
        );
        let mut account = stored_account(Some("org-1"), AccountPlan::Pro);

        let mut out = Vec::new();
        apply_refreshed_plan(&mut account, &orgs, "Person 2", &mut out);

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "  Updated plan: Person 2 (Pro -> Max)\n"
        );
    }

    // -- portal_app_id_candidates ------------------------------------------

    fn profile(browser: Browser, keyring_app: &str) -> DiscoveredProfile {
        DiscoveredProfile {
            browser,
            keyring_app: keyring_app.into(),
            profile_dir: "Default".into(),
            display_name: None,
            google_email: None,
            cookies_db: PathBuf::from("/nonexistent/Cookies"),
        }
    }

    #[test]
    fn portal_candidates_are_the_empty_host_app_id_then_the_flathub_id() {
        let got = portal_app_id_candidates(&profile(Browser::Chrome, "chrome"));
        assert_eq!(got, vec!["", "com.google.Chrome"]);
    }

    #[test]
    fn portal_candidates_cover_each_linux_browser_and_skip_arc() {
        assert_eq!(
            portal_app_id_candidates(&profile(Browser::Brave, "brave")),
            // Brave is the only supported browser with an official snap, so it
            // alone gets the `snap.<name>` id xdg-desktop-portal assigns.
            vec!["", "com.brave.Browser", "snap.brave"]
        );
        assert_eq!(
            portal_app_id_candidates(&profile(Browser::Edge, "microsoft-edge")),
            vec!["", "com.microsoft.Edge"]
        );
        // Arc has no Linux build, so it contributes no Flatpak id; the keyring
        // `application` name is never a portal app_id (see the fn doc).
        assert_eq!(
            portal_app_id_candidates(&profile(Browser::Arc, "arc")),
            vec![""]
        );
    }

    // -- scan_resolving_portal ---------------------------------------------

    const RIGHT: &[u8] = b"the-real-portal-secret";
    const WRONG: &[u8] = b"a-secret-for-another-app";

    fn found(session_key: &str) -> ProfileScan {
        ProfileScan::Found {
            session_key: session_key.into(),
            org_id: None,
        }
    }

    /// A scan that behaves like a v12 profile: unreadable without a portal
    /// secret, unreadable with the wrong one, readable with `RIGHT`.
    fn v12_scan(sources: &KeySources) -> ProfileScan {
        match sources.portal.as_deref() {
            None => ProfileScan::NoPortalSecret,
            Some(s) if s == RIGHT => found("sk-ant-sid01-V12"),
            Some(_) => ProfileScan::NoSession,
        }
    }

    #[test]
    fn no_portal_lookup_happens_when_the_profile_has_no_v12_cookie() {
        // Laziness matters: a v10/v11 user must never be made to unlock a
        // keyring for a secret their cookies do not use.
        let fetched: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let got = scan_resolving_portal(
            &KeySources::keyring(Some("safe-storage".into())),
            &["".to_string(), "com.google.Chrome".to_string()],
            |app_id| {
                fetched.borrow_mut().push(app_id.to_string());
                Some(RIGHT.to_vec())
            },
            |_| found("sk-ant-sid01-V11"),
        );
        assert!(matches!(got, ProfileScan::Found { ref session_key, .. } if session_key == "sk-ant-sid01-V11"));
        assert!(fetched.borrow().is_empty(), "fetched {:?}", fetched.borrow());
    }

    #[test]
    fn a_profile_without_a_session_does_not_trigger_a_portal_lookup() {
        let fetched: RefCell<usize> = RefCell::new(0);
        let got = scan_resolving_portal(
            &KeySources::default(),
            &["".to_string()],
            |_| {
                *fetched.borrow_mut() += 1;
                Some(RIGHT.to_vec())
            },
            |_| ProfileScan::NoSession,
        );
        assert!(matches!(got, ProfileScan::NoSession));
        assert_eq!(*fetched.borrow(), 0);
    }

    #[test]
    fn candidates_are_tried_in_order_until_one_decrypts() {
        let fetched: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let got = scan_resolving_portal(
            &KeySources::default(),
            &[
                "".to_string(),
                "com.google.Chrome".to_string(),
                "chrome".to_string(),
            ],
            |app_id| {
                fetched.borrow_mut().push(app_id.to_string());
                match app_id {
                    "" => None,                              // no such keyring item
                    "com.google.Chrome" => Some(WRONG.to_vec()), // decrypts to nothing
                    _ => Some(RIGHT.to_vec()),
                }
            },
            v12_scan,
        );
        assert!(matches!(got, ProfileScan::Found { ref session_key, .. } if session_key == "sk-ant-sid01-V12"));
        assert_eq!(*fetched.borrow(), vec!["", "com.google.Chrome", "chrome"]);
    }

    #[test]
    fn the_profile_is_skipped_when_no_candidate_decrypts_it() {
        let got = scan_resolving_portal(
            &KeySources::default(),
            &["".to_string(), "com.google.Chrome".to_string()],
            |_| Some(WRONG.to_vec()),
            v12_scan,
        );
        assert!(matches!(got, ProfileScan::NoPortalSecret));
    }

    #[test]
    fn an_empty_candidate_list_leaves_a_v12_profile_skipped() {
        let got = scan_resolving_portal(&KeySources::default(), &[], |_| None, v12_scan);
        assert!(matches!(got, ProfileScan::NoPortalSecret));
    }

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
