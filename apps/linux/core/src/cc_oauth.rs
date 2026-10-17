//! Claude Code's own OAuth client, the pure parts (port of macOS `ClaudeCodeOAuthClient` and
//! `PKCE`): the authorize URL `/login` opens, the token request body, the mapping of the token
//! and profile responses to the `claudeAiOauth` + `oauthAccount` pair `/login` stores, and the
//! loopback callback parsing. Network calls live in the app.

use serde_json::{json, Value};

use crate::cc_switch::model::{canonical, OAuthCredential, VaultEntry};

pub const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const AUTHORIZE_ENDPOINT: &str = "https://claude.com/cai/oauth/authorize";
pub const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
pub const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
pub const SCOPE: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload user:plugins";
/// Claude Code's fallback when the token response has no `refresh_token_expires_in`.
pub const DEFAULT_REFRESH_LIFETIME_S: i64 = 30 * 86_400;

/// A PKCE verifier plus the OAuth `state`, as Claude Code makes them: 32 random bytes each,
/// base64url without padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub state: String,
}

impl Pkce {
    pub fn make() -> Self {
        Pkce { verifier: random_b64(), state: random_b64() }
    }

    /// base64url(SHA-256(verifier)).
    pub fn challenge(&self) -> String {
        use sha2::Digest;
        b64url(&sha2::Sha256::digest(self.verifier.as_bytes()))
    }
}

fn b64url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random_b64() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("OS random source");
    b64url(&bytes)
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (b'+', ..) => {
                out.push(b' ');
                i += 1;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The consent page `/login` opens, with the query in Claude Code's order.
pub fn authorize_url(p: &Pkce, redirect_uri: &str, login_hint: Option<&str>) -> String {
    let challenge = p.challenge();
    let mut params: Vec<(&str, &str)> = vec![
        ("code", "true"),
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", SCOPE),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("state", &p.state),
    ];
    if let Some(hint) = login_hint {
        params.push(("login_hint", hint));
    }
    let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect();
    format!("{AUTHORIZE_ENDPOINT}?{}", query.join("&"))
}

/// The authorization-code token request.
pub fn token_body(code: &str, p: &Pkce, redirect_uri: &str) -> Value {
    json!({
        "grant_type": "authorization_code", "code": code, "redirect_uri": redirect_uri,
        "client_id": CLIENT_ID, "code_verifier": p.verifier, "state": p.state,
    })
}

/// Claude Code's mapping of `organization.organization_type`.
pub fn subscription_type(organization_type: &str) -> Option<&'static str> {
    match organization_type {
        "claude_max" => Some("max"),
        "claude_pro" => Some("pro"),
        "claude_enterprise" => Some("enterprise"),
        "claude_team" => Some("team"),
        _ => None,
    }
}

/// The token and profile responses as the vault entry `/login` would store, issued at
/// `issued_ms`. `None` when a required field is missing.
pub fn entry_from(token: &Value, profile: &Value, issued_ms: i64) -> Option<VaultEntry> {
    let access = token.get("access_token")?.as_str()?;
    let refresh = token.get("refresh_token")?.as_str()?;
    let expires_in = token.get("expires_in")?.as_f64()?;
    let empty = json!({});
    let account = profile.get("account").filter(|a| a.is_object()).unwrap_or(&empty);
    let org = profile.get("organization").filter(|o| o.is_object()).unwrap_or(&empty);
    let email = account.get("email")?.as_str()?;

    let refresh_lifetime = token
        .get("refresh_token_expires_in")
        .and_then(Value::as_f64)
        .unwrap_or(DEFAULT_REFRESH_LIFETIME_S as f64);
    let scopes: Vec<&str> = token.get("scope").and_then(Value::as_str).map_or(Vec::new(), |s| s.split(' ').collect());
    let mut oauth = json!({
        "accessToken": access,
        "refreshToken": refresh,
        "expiresAt": issued_ms + (expires_in * 1000.0) as i64,
        "refreshTokenExpiresAt": issued_ms + (refresh_lifetime * 1000.0) as i64,
        "scopes": scopes,
    });
    if let Some(sub) = org.get("organization_type").and_then(Value::as_str).and_then(subscription_type) {
        oauth["subscriptionType"] = json!(sub);
    }
    if let Some(tier) = org.get("rate_limit_tier").and_then(Value::as_str) {
        oauth["rateLimitTier"] = json!(tier);
    }

    let mut oauth_account = json!({
        "accountUuid": account.get("uuid").cloned().unwrap_or(Value::Null),
        "emailAddress": email,
        "organizationUuid": org.get("uuid").cloned().unwrap_or(Value::Null),
        "profileFetchedAt": issued_ms,
    });
    let copies: [(&str, &Value, &str); 10] = [
        ("displayName", account, "display_name"),
        ("fullName", account, "full_name"),
        ("accountCreatedAt", account, "created_at"),
        ("organizationName", org, "name"),
        ("organizationType", org, "organization_type"),
        ("billingType", org, "billing_type"),
        ("organizationRateLimitTier", org, "rate_limit_tier"),
        ("seatTier", org, "seat_tier"),
        ("hasExtraUsageEnabled", org, "has_extra_usage_enabled"),
        ("subscriptionCreatedAt", org, "subscription_created_at"),
    ];
    for (key, source, field) in copies {
        if let Some(v) = source.get(field) {
            oauth_account[key] = v.clone();
        }
    }
    Some(VaultEntry { oauth: OAuthCredential::from_value(&oauth)?, oauth_account: canonical(&oauth_account)? })
}

/// The `code` of an HTTP request `GET /callback?code=...&state=...`, only when `state` matches.
pub fn callback_code(request: &str, expected_state: &str) -> Option<String> {
    let line = request.lines().next()?;
    let target = line.split(' ').nth(1)?;
    let (path, query) = target.split_once('?')?;
    if path != "/callback" {
        return None;
    }
    let param = |name: &str| {
        query.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == name).then(|| decode(v))
        })
    };
    if param("state")? != expected_state {
        return None;
    }
    param("code")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_is_sha256_base64url() {
        // RFC 7636, appendix B.
        let p = Pkce { verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into(), state: "s".into() };
        assert_eq!(p.challenge(), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let a = Pkce::make();
        let b = Pkce::make();
        assert_eq!(a.verifier.len(), 43);
        assert_eq!(a.state.len(), 43);
        assert!(a.verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(a, b);
    }

    #[test]
    fn authorize_url_has_all_params() {
        let p = Pkce { verifier: "v".into(), state: "st".into() };
        let u = authorize_url(&p, "http://localhost:4321/callback", Some("a+b@x.com"));
        assert!(u.starts_with("https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&redirect_uri=http"));
        assert!(u.contains("redirect_uri=http%3A%2F%2Flocalhost%3A4321%2Fcallback"));
        assert!(u.contains("&scope=user%3Aprofile%20user%3Ainference%20"));
        assert!(u.contains(&format!("&code_challenge={}&code_challenge_method=S256&state=st", p.challenge())));
        assert!(u.ends_with("&login_hint=a%2Bb%40x.com"));
        assert!(!authorize_url(&p, "r", None).contains("login_hint"));
    }

    fn token() -> Value {
        json!({"access_token": "at", "refresh_token": "rt", "expires_in": 28_800,
               "scope": "user:profile user:inference"})
    }

    fn profile() -> Value {
        json!({
            "account": {"uuid": "acc-1", "email": "a@x.com", "display_name": "A", "full_name": "A B", "created_at": "2025"},
            "organization": {"uuid": "org-1", "name": "Org", "organization_type": "claude_max", "billing_type": "stripe",
                             "rate_limit_tier": "default_claude_max_20x", "seat_tier": null,
                             "has_extra_usage_enabled": false, "subscription_created_at": "2025-01"}
        })
    }

    #[test]
    fn entry_maps_token_and_profile() {
        let e = entry_from(&token(), &profile(), 1_000_000).unwrap();
        let o = e.oauth.value();
        assert_eq!(o["accessToken"], "at");
        assert_eq!(o["refreshToken"], "rt");
        assert_eq!(o["expiresAt"], 1_000_000 + 28_800_000);
        assert_eq!(o["refreshTokenExpiresAt"], 1_000_000 + 30 * 86_400_000_i64);
        assert_eq!(o["scopes"], json!(["user:profile", "user:inference"]));
        assert_eq!(o["subscriptionType"], "max");
        assert_eq!(o["rateLimitTier"], "default_claude_max_20x");
        let a: Value = serde_json::from_str(&e.oauth_account).unwrap();
        assert_eq!(a["emailAddress"], "a@x.com");
        assert_eq!(a["accountUuid"], "acc-1");
        assert_eq!(a["organizationUuid"], "org-1");
        assert_eq!(a["organizationType"], "claude_max");
        assert_eq!(a["displayName"], "A");
        assert_eq!(a["profileFetchedAt"], 1_000_000);
        let mut t = token();
        t["refresh_token_expires_in"] = json!(100);
        let e = entry_from(&t, &profile(), 0).unwrap();
        assert_eq!(e.oauth.refresh_expires_at_ms, Some(100_000));
        assert_eq!(subscription_type("claude_pro"), Some("pro"));
        assert_eq!(subscription_type("other"), None);
    }

    #[test]
    fn entry_requires_email() {
        let mut p = profile();
        p["account"]["email"] = Value::Null;
        assert!(entry_from(&token(), &p, 0).is_none());
        assert!(entry_from(&json!({"access_token": "at"}), &profile(), 0).is_none());
    }

    #[test]
    fn callback_ignores_wrong_state() {
        assert_eq!(callback_code("GET /callback?code=abc&state=other HTTP/1.1\r\nHost: x\r\n\r\n", "st"), None);
        assert_eq!(callback_code("GET /favicon.ico HTTP/1.1\r\n\r\n", "st"), None);
        assert_eq!(callback_code("GET /other?code=abc&state=st HTTP/1.1\r\n\r\n", "st"), None);
    }

    #[test]
    fn callback_reads_code() {
        assert_eq!(
            callback_code("GET /callback?code=a%2Fb%3D&state=st HTTP/1.1\r\nHost: localhost\r\n\r\n", "st").as_deref(),
            Some("a/b=")
        );
    }

    #[test]
    fn entry_credential_parses() {
        let e = entry_from(&token(), &profile(), 5).unwrap();
        assert_eq!(OAuthCredential::from_value(&e.oauth.value()), Some(e.oauth.clone()));
        assert!(canonical(&serde_json::from_str::<Value>(&e.oauth_account).unwrap()).is_some());
    }
}
