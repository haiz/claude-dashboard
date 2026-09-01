//! `claude-dashboard-helper usage <orgId> <sessionKey>` — a **passthrough**
//! subcommand: it does not parse or reshape the upstream payload in any way.
//!
//! Mirrors `apps/macos/Helper/UsageCommand.swift`; see
//! `contract/helper-cli.md` "usage" for the exact stderr text, arg-count
//! requirement, and byte-for-byte body-printing rule this reproduces
//! verbatim. Per `core::api`'s module doc (Ruling B), `ApiError` has no
//! `AuthExpired` variant here, so `401`/`403` fall through to `HttpError`
//! and print as `HTTP 401` / `HTTP 403` like any other non-2xx status —
//! matching the Swift helper, which never applies the GUI's
//! expired-account mapping.

use claude_dashboard_core::api::{usage_raw, ApiError};

/// Runs the `usage` subcommand and returns its process exit code.
pub fn run_usage(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("Usage: claude-dashboard-helper usage <orgId> <sessionKey>");
        return 1;
    }
    let (org_id, session_key) = (&args[0], &args[1]);

    // The refreshed `new_session_key` is deliberately ignored: `usage` is a
    // pure passthrough that persists nothing, matching
    // `UsageCommand.swift`.
    match usage_raw(org_id, session_key) {
        Ok(resp) => {
            if resp.body.is_empty() {
                eprintln!("Empty response.");
                return 1;
            }
            // Byte-for-byte passthrough: `print!`, not `println!`, so no
            // trailing newline is appended to the upstream body.
            print!("{}", resp.body);
            0
        }
        Err(ApiError::InvalidOrgId) => {
            eprintln!("Invalid orgId.");
            1
        }
        Err(ApiError::HttpError(status)) => {
            eprintln!("HTTP {status}");
            1
        }
        Err(ApiError::Timeout) => {
            eprintln!("Request timed out.");
            1
        }
        Err(ApiError::Network(desc)) => {
            eprintln!("Network error: {desc}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_args_returns_1() {
        assert_eq!(run_usage(&[]), 1);
    }

    #[test]
    fn one_arg_returns_1() {
        assert_eq!(run_usage(&["only-one".to_string()]), 1);
    }
}
