//! Detects Claude sessions in Windows browser profiles.
//!
//! [`classify_profile`] is the pure decision; [`scan_windows_profiles`] wires
//! the existing discovery and cookie primitives into it.

use crate::cookie::CookieError;
use crate::model::Browser;

#[derive(Debug, Clone, PartialEq)]
pub struct ScannedSession {
    pub browser: Browser,
    pub profile_dir: String,
    pub display_name: String,
    pub google_email: Option<String>,
    pub session_key: String,
    pub org_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileScanStatus {
    Found(ScannedSession),
    /// A cookie is `v20` app-bound: the whole profile needs the extension.
    AppBound,
    NoSession,
}

/// Pure classification of one profile's decoded cookies. Any app-bound cookie
/// makes the whole profile `AppBound`; otherwise `sessionKey` decides, with
/// `lastActiveOrg` as the optional org id. Other cookies and errors are ignored.
pub fn classify_profile(
    browser: Browser,
    profile_dir: &str,
    display_name: &str,
    google_email: Option<String>,
    cookies: &[(String, Result<String, CookieError>)],
) -> ProfileScanStatus {
    if cookies
        .iter()
        .any(|(_, r)| matches!(r, Err(CookieError::AppBoundEncrypted)))
    {
        return ProfileScanStatus::AppBound;
    }
    let ok_value = |name: &str| {
        cookies
            .iter()
            .find(|(n, r)| n == name && r.is_ok())
            .and_then(|(_, r)| r.as_ref().ok().cloned())
    };
    match ok_value("sessionKey") {
        Some(session_key) => ProfileScanStatus::Found(ScannedSession {
            browser,
            profile_dir: profile_dir.to_string(),
            display_name: display_name.to_string(),
            google_email,
            session_key,
            org_id: ok_value("lastActiveOrg"),
        }),
        None => ProfileScanStatus::NoSession,
    }
}

/// Scans every Chrome/Edge/Brave profile under `%LOCALAPPDATA%`; one status per
/// discovered profile.
#[cfg(windows)]
pub fn scan_windows_profiles() -> Vec<ProfileScanStatus> {
    use crate::browser::{discover_windows_profiles_under, read_claude_cookie_db};
    use crate::cookie::win::{decode_value, profile_key};

    let local = match std::env::var("LOCALAPPDATA") {
        Ok(v) if !v.is_empty() => std::path::PathBuf::from(v),
        _ => return Vec::new(),
    };
    discover_windows_profiles_under(&local)
        .into_iter()
        .map(|p| {
            let (Some((schema, raw)), Some(key)) =
                (read_claude_cookie_db(&p.cookies_db), profile_key(&p.local_state))
            else {
                return ProfileScanStatus::NoSession;
            };
            let cookies: Vec<(String, Result<String, CookieError>)> = raw
                .iter()
                .map(|c| {
                    (
                        c.name.clone(),
                        decode_value(&c.encrypted_value, &c.host_key, schema, &key),
                    )
                })
                .collect();
            let name = p.display_name.clone().unwrap_or_else(|| p.profile_dir.clone());
            classify_profile(p.browser, &p.profile_dir, &name, p.google_email, &cookies)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(n: &str, v: &str) -> (String, Result<String, CookieError>) {
        (n.to_string(), Ok(v.to_string()))
    }
    fn err(n: &str, e: CookieError) -> (String, Result<String, CookieError>) {
        (n.to_string(), Err(e))
    }
    fn classify(c: &[(String, Result<String, CookieError>)]) -> ProfileScanStatus {
        classify_profile(Browser::Chrome, "Default", "Me", Some("a@b.c".into()), c)
    }

    #[test]
    fn found_with_org_echoes_fields() {
        let s = classify(&[ok("sessionKey", "sk-x"), ok("lastActiveOrg", "org-1")]);
        assert_eq!(
            s,
            ProfileScanStatus::Found(ScannedSession {
                browser: Browser::Chrome,
                profile_dir: "Default".into(),
                display_name: "Me".into(),
                google_email: Some("a@b.c".into()),
                session_key: "sk-x".into(),
                org_id: Some("org-1".into()),
            })
        );
    }

    #[test]
    fn found_without_org() {
        match classify(&[ok("sessionKey", "sk-x")]) {
            ProfileScanStatus::Found(s) => assert_eq!(s.org_id, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn app_bound_wins_even_with_session_key() {
        let s = classify(&[ok("sessionKey", "sk-x"), err("cf_clearance", CookieError::AppBoundEncrypted)]);
        assert_eq!(s, ProfileScanStatus::AppBound);
    }

    #[test]
    fn no_session_key_is_no_session() {
        assert_eq!(classify(&[ok("lastActiveOrg", "o")]), ProfileScanStatus::NoSession);
        assert_eq!(classify(&[]), ProfileScanStatus::NoSession);
    }

    #[test]
    fn session_key_non_app_bound_error_is_no_session() {
        assert_eq!(
            classify(&[err("sessionKey", CookieError::DecryptFailed)]),
            ProfileScanStatus::NoSession
        );
    }

    #[test]
    fn unrelated_error_is_ignored() {
        let s = classify(&[err("other", CookieError::BadUtf8), ok("sessionKey", "sk-x")]);
        assert!(matches!(s, ProfileScanStatus::Found(_)));
    }
}
