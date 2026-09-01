//! HTTP client for claude.ai's usage and organizations endpoints.
//!
//! Mirrors `apps/macos/Shared/UsageAPIService.swift`; see
//! `contract/README.md`'s "Session-key refresh and auth expiry" section for
//! the request shape and the `validateResponse`/`parseSessionKey` rules this
//! module reproduces.
//!
//! **Intentional divergence from that contract section (CONTROLLER RULING
//! B, Linux port plan B, task 7):** the macOS app maps HTTP `401`/`403` to
//! `UsageAPIError.authExpired`, which the view model turns into an
//! account's `expired` status. This helper has no such status to write —
//! `sync` just skips an account on any error, and the `usage` subcommand
//! prints `HTTP <status>` verbatim. So [`ApiError`] here has **no**
//! `AuthExpired` variant; `401` and `403` fall through to
//! [`ApiError::HttpError`] like every other non-2xx status, preserving the
//! real numeric code. Do not "fix" this back to match the contract — the
//! 401/403-to-account-expired write-back is GUI behaviour reserved for a
//! later plan. Every other rule in that contract section (request headers,
//! the 15s timeout, `Set-Cookie` parsing) still applies verbatim.

use std::error::Error as _;
use std::io::Read;
use std::time::Duration;

/// Errors from a call to the claude.ai HTTP API.
///
/// See the module doc for why this has no `AuthExpired` variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// `org_id` contains a character that cannot form a single, unambiguous
    /// URL path segment.
    InvalidOrgId,
    /// A response was received with a status outside `200..=299`, including
    /// `401`/`403`. The numeric status is preserved verbatim so the `usage`
    /// subcommand can print `HTTP <status>`.
    HttpError(u16),
    /// A connection, DNS, or other I/O failure. Holds a human-readable
    /// description (`Display` of the underlying transport error).
    Network(String),
    /// The request exceeded the 15-second timeout.
    Timeout,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::InvalidOrgId => write!(f, "org_id is not a valid URL path segment"),
            ApiError::HttpError(status) => write!(f, "HTTP {status}"),
            ApiError::Network(desc) => write!(f, "network error: {desc}"),
            ApiError::Timeout => write!(f, "request timed out"),
        }
    }
}

impl std::error::Error for ApiError {}

/// Successful response from [`usage_raw`].
pub struct UsageResponse {
    pub body: String,
    /// The new session key parsed from the response's `Set-Cookie`
    /// header(s), if the server rotated it. `None` if absent.
    pub new_session_key: Option<String>,
}

/// Overall request timeout (connect + read), per `contract/README.md`.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Headers sent on every request, per `contract/README.md`'s "Session-key
/// refresh and auth expiry" section. `Cookie` is added separately since its
/// value is per-call.
const REQUEST_HEADERS: [(&str, &str); 3] = [
    ("accept", "*/*"),
    ("content-type", "application/json"),
    ("anthropic-client-platform", "web_claude_ai"),
];

/// Parses the `sessionKey=` cookie out of one or more raw `Set-Cookie`
/// response header values.
///
/// Each header value is split on `;`, each component is trimmed, and the
/// first component beginning with the literal prefix `sessionKey=` has that
/// prefix stripped and is returned **verbatim** — not URL-decoded, not
/// unquoted, no length or format check. Searches across all header values
/// given (a server may send several separate `Set-Cookie` headers; ureq
/// does not collapse them into one — see task-7-report.md). `None` if no
/// component matches.
pub fn parse_session_key(set_cookie_values: &[String]) -> Option<String> {
    for header in set_cookie_values {
        for component in header.split(';') {
            let trimmed = component.trim();
            if let Some(rest) = trimmed.strip_prefix("sessionKey=") {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// Maps an HTTP status code to a result. Per Ruling B (see module doc):
/// only `200..=299` is `Ok`; every other status — `401`/`403` included —
/// becomes `Err(ApiError::HttpError(status))` with no special case.
fn map_status(status: u16) -> Result<(), ApiError> {
    if (200..=299).contains(&status) {
        Ok(())
    } else {
        Err(ApiError::HttpError(status))
    }
}

/// Rejects `org_id` values that cannot form a single, unambiguous URL path
/// segment: empty, or containing `/` (adds a path segment), `?` (starts a
/// query string), `#` (starts a fragment, never sent to the server), or any
/// control/whitespace byte.
fn validate_org_id(org_id: &str) -> Result<(), ApiError> {
    let has_illegal_char = org_id
        .chars()
        .any(|c| matches!(c, '/' | '?' | '#') || c.is_control() || c.is_whitespace());
    if org_id.is_empty() || has_illegal_char {
        Err(ApiError::InvalidOrgId)
    } else {
        Ok(())
    }
}

/// Classifies an `io::ErrorKind` as a timeout. `TimedOut` is ureq's normal
/// path (its `DeadlineStream` raises exactly this kind — see
/// task-7-report.md); `WouldBlock` is included because a blocking client's
/// expired `SO_RCVTIMEO` can surface as `EWOULDBLOCK` on some platforms, and
/// in a blocking client `WouldBlock` has no other meaning.
fn is_timeout_kind(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

/// Classifies a transport-level (non-HTTP-status) ureq error into
/// [`ApiError::Timeout`] or [`ApiError::Network`].
fn classify_transport_error(err: ureq::Transport) -> ApiError {
    let is_timeout = err
        .source()
        .and_then(|s| s.downcast_ref::<std::io::Error>())
        .map(|ioe| is_timeout_kind(ioe.kind()))
        .unwrap_or(false);
    if is_timeout {
        ApiError::Timeout
    } else {
        ApiError::Network(err.to_string())
    }
}

/// Issues the shared GET request shape (headers, `Cookie: sessionKey=`,
/// timeout) and returns the raw response body bytes together with every
/// raw `Set-Cookie` header value on success. Non-2xx statuses (via either
/// ureq's own `Error::Status` for `>= 400`, or [`map_status`] for the
/// `1xx`/`3xx` range ureq treats as `Ok`) become
/// `Err(ApiError::HttpError(status))`, discarding any `Set-Cookie` on the
/// error response — matching the contract's
/// `validateResponse`-before-`parseSessionKey` ordering. `Set-Cookie` is
/// read off the response before its body reader is consumed: headers are
/// already parsed by the time `call()` returns, and `into_reader` only
/// consumes the body stream, so the read order below does not matter for
/// correctness — but keeping cookie extraction textually first documents
/// the dependency.
fn perform_get_bytes(url: &str, session_key: &str) -> Result<(Vec<u8>, Vec<String>), ApiError> {
    let mut req = ureq::get(url).timeout(REQUEST_TIMEOUT);
    for (name, value) in REQUEST_HEADERS {
        req = req.set(name, value);
    }
    req = req.set("Cookie", &format!("sessionKey={session_key}"));

    let resp = match req.call() {
        Ok(resp) => resp,
        Err(ureq::Error::Status(code, _)) => return Err(ApiError::HttpError(code)),
        Err(ureq::Error::Transport(t)) => return Err(classify_transport_error(t)),
    };

    map_status(resp.status())?;
    let cookies = resp
        .all("set-cookie")
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let mut body = Vec::new();
    resp.into_reader()
        .read_to_end(&mut body)
        .map_err(|e| ApiError::Network(e.to_string()))?;
    Ok((body, cookies))
}

/// [`perform_get_bytes`], with the body lossily decoded as UTF-8 (invalid
/// sequences become U+FFFD) — the same decoding
/// `ureq::Response::into_string` performs without the (unused) `charset`
/// feature. Used by [`fetch_organizations`], which has no non-UTF-8
/// contract case to honor, so its existing (lossy) behavior is preserved
/// unchanged.
fn perform_get(url: &str, session_key: &str) -> Result<(String, Vec<String>), ApiError> {
    let (bytes, cookies) = perform_get_bytes(url, session_key)?;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), cookies))
}

/// Strictly decodes a response body as UTF-8, per `contract/helper-cli.md`
/// "usage": an invalid-UTF-8 body is treated as *absent*, not lossily
/// repaired — mirroring `UsageCommand.swift`'s `String(data:encoding:
/// .utf8)`, which returns `nil` on the same input and whose caller then
/// prints `Empty response.\n`. Returning `""` here lets `usage_raw`'s
/// caller's existing `resp.body.is_empty()` check reproduce that same
/// mapping, with no new `ApiError` variant needed.
fn decode_body_strict(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_default()
}

/// Fetches `/api/organizations/{org_id}/usage`, the 5-hour/7-day/Fable
/// usage payload that [`crate::usage`] decodes.
pub fn usage_raw(org_id: &str, session_key: &str) -> Result<UsageResponse, ApiError> {
    validate_org_id(org_id)?;
    let url = format!("https://claude.ai/api/organizations/{org_id}/usage");
    let (bytes, cookies) = perform_get_bytes(&url, session_key)?;
    Ok(UsageResponse {
        body: decode_body_strict(bytes),
        new_session_key: parse_session_key(&cookies),
    })
}

/// Fetches `/api/organizations`, whose `capabilities` field a later task
/// (plan-tier detection) reads. Does **not** parse a session key from this
/// response — the Swift client discards it here too (see module doc).
pub fn fetch_organizations(session_key: &str) -> Result<String, ApiError> {
    let (body, _cookies) = perform_get("https://claude.ai/api/organizations", session_key)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- parse_session_key (brief's Step 1 tests, verbatim) -----------------

    #[test]
    fn parses_first_matching_component() {
        let hdr = vec!["foo=bar; sessionKey=sk-abc123; Path=/; HttpOnly".to_string()];
        assert_eq!(parse_session_key(&hdr).as_deref(), Some("sk-abc123"));
    }

    #[test]
    fn none_when_absent() {
        assert_eq!(parse_session_key(&["other=1; Path=/".to_string()]), None);
    }

    #[test]
    fn finds_key_across_multiple_set_cookie_headers() {
        let hdrs = vec![
            "a=1; Path=/".to_string(),
            "sessionKey=sk-xyz; Secure".to_string(),
        ];
        assert_eq!(parse_session_key(&hdrs).as_deref(), Some("sk-xyz"));
    }

    #[test]
    fn value_taken_verbatim_not_url_decoded() {
        let hdr = vec!["sessionKey=sk-a%20b".to_string()];
        assert_eq!(parse_session_key(&hdr).as_deref(), Some("sk-a%20b"));
    }

    // -- map_status -----------------------------------------------------------

    #[test]
    fn map_status_401_is_http_error_not_auth_expired() {
        // Ruling B: the helper never special-cases 401/403 into an
        // AuthExpired variant -- it preserves the real status.
        assert_eq!(map_status(401), Err(ApiError::HttpError(401)));
    }

    #[test]
    fn map_status_403_is_http_error_not_auth_expired() {
        assert_eq!(map_status(403), Err(ApiError::HttpError(403)));
    }

    #[test]
    fn map_status_200_is_ok() {
        assert_eq!(map_status(200), Ok(()));
    }

    #[test]
    fn map_status_299_is_ok() {
        assert_eq!(map_status(299), Ok(()));
    }

    #[test]
    fn map_status_300_is_http_error() {
        assert_eq!(map_status(300), Err(ApiError::HttpError(300)));
    }

    #[test]
    fn map_status_199_is_http_error() {
        assert_eq!(map_status(199), Err(ApiError::HttpError(199)));
    }

    // -- validate_org_id --------------------------------------------------

    #[test]
    fn validate_org_id_accepts_uuid_shaped_id() {
        assert_eq!(
            validate_org_id("3b8c3678-3a00-425c-8d22-22bca37ae65b"),
            Ok(())
        );
    }

    #[test]
    fn validate_org_id_rejects_empty() {
        assert_eq!(validate_org_id(""), Err(ApiError::InvalidOrgId));
    }

    #[test]
    fn validate_org_id_rejects_embedded_slash() {
        assert_eq!(validate_org_id("abc/def"), Err(ApiError::InvalidOrgId));
    }

    #[test]
    fn validate_org_id_rejects_query_marker() {
        assert_eq!(validate_org_id("abc?def"), Err(ApiError::InvalidOrgId));
    }

    #[test]
    fn validate_org_id_rejects_fragment_marker() {
        assert_eq!(validate_org_id("abc#def"), Err(ApiError::InvalidOrgId));
    }

    #[test]
    fn validate_org_id_rejects_whitespace() {
        assert_eq!(validate_org_id("abc def"), Err(ApiError::InvalidOrgId));
    }

    #[test]
    fn validate_org_id_rejects_control_character() {
        assert_eq!(validate_org_id("abc\ndef"), Err(ApiError::InvalidOrgId));
    }

    // -- is_timeout_kind ----------------------------------------------------

    #[test]
    fn timed_out_kind_is_timeout() {
        assert!(is_timeout_kind(std::io::ErrorKind::TimedOut));
    }

    #[test]
    fn would_block_kind_is_timeout() {
        // Blocking ureq surfaces an expired SO_RCVTIMEO as EWOULDBLOCK on
        // some platforms; in a blocking client WouldBlock has no other
        // meaning, so treat it as a timeout too.
        assert!(is_timeout_kind(std::io::ErrorKind::WouldBlock));
    }

    #[test]
    fn connection_reset_kind_is_not_timeout() {
        assert!(!is_timeout_kind(std::io::ErrorKind::ConnectionReset));
    }

    // -- decode_body_strict ------------------------------------------------

    #[test]
    fn decode_body_strict_valid_utf8_roundtrips() {
        assert_eq!(decode_body_strict(b"hello world".to_vec()), "hello world");
    }

    #[test]
    fn decode_body_strict_invalid_utf8_is_empty() {
        // 0xff, 0xfe is not valid UTF-8 in any position; per
        // `contract/helper-cli.md` "usage" this must NOT be lossily
        // repaired (that would silently deviate from the contract's
        // "Empty or non-UTF8 body -> Empty response." bucket) -- it must
        // come back empty so the caller's is_empty() check fires.
        assert_eq!(decode_body_strict(vec![0xff, 0xfe]), "");
    }

    #[test]
    fn decode_body_strict_empty_bytes_is_empty() {
        assert_eq!(decode_body_strict(Vec::new()), "");
    }
}
