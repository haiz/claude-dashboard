//! `usage` driven as a real process against a loopback server.
//! `contract/helper-cli.md` "usage" is the source of truth for every string here.

mod support;

use claude_dashboard_core::api::BASE_URL_OVERRIDE_VAR;
use support::{run_helper, LoopbackServer, Response, USAGE_WILDCARD};

fn run_usage(server: &LoopbackServer, org_id: &str) -> support::Output {
    // Bound to a local: `&server.origin()` in the slice literal is a `&String`
    // where a `&str` is required.
    let origin = server.origin();
    run_helper(
        &["usage", org_id, "sk-fake-test-key"],
        None,
        &[(BASE_URL_OVERRIDE_VAR, origin.as_str())],
    )
}

#[test]
fn successful_request_is_passed_through_and_carries_the_contract_headers() {
    let server = LoopbackServer::start();
    let body = r#"{"five_hour":{"utilization":12},"unknown_field":"kept"}"#;
    server.respond(USAGE_WILDCARD, Response::json(body));

    let out = run_usage(&server, "org-abc");

    assert_eq!(out.code, 0);
    assert_eq!(out.stdout_text(), body);
    assert_eq!(out.stderr, "");

    let recorded = server.recorded();
    let request = recorded.first().expect("one request");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/api/organizations/org-abc/usage");
    assert_eq!(request.headers["accept"], "*/*");
    assert_eq!(request.headers["content-type"], "application/json");
    assert_eq!(request.headers["anthropic-client-platform"], "web_claude_ai");
    assert_eq!(request.headers["cookie"], "sessionKey=sk-fake-test-key");
}

#[test]
fn empty_body_is_reported_as_empty_response() {
    let server = LoopbackServer::start();
    server.respond(USAGE_WILDCARD, Response::status(200));

    let out = run_usage(&server, "org-1");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "Empty response.\n");
    assert!(out.stdout.is_empty());
}

#[test]
fn non_utf8_body_is_reported_as_empty_response() {
    let server = LoopbackServer::start();
    server.respond(
        USAGE_WILDCARD,
        Response::Reply { status: 200, headers: Vec::new(), body: vec![0xff, 0xfe] },
    );

    let out = run_usage(&server, "org-1");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "Empty response.\n");
}

#[test]
fn status_401_is_printed_verbatim() {
    // Ruling B: no AuthExpired mapping in the helper, the real status survives.
    let server = LoopbackServer::start();
    server.respond(USAGE_WILDCARD, Response::status(401));

    let out = run_usage(&server, "org-1");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "HTTP 401\n");
}

#[test]
fn status_500_is_printed_verbatim() {
    let server = LoopbackServer::start();
    server.respond(USAGE_WILDCARD, Response::status(500));

    let out = run_usage(&server, "org-1");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "HTTP 500\n");
}

#[test]
fn connection_closed_mid_request_is_a_network_error() {
    let server = LoopbackServer::start();
    server.respond(USAGE_WILDCARD, Response::CloseImmediately);

    let out = run_usage(&server, "org-1");

    assert_eq!(out.code, 1);
    // Only the prefix is contract; the rest is ureq's Display.
    assert!(out.stderr.starts_with("Network error:"), "unexpected stderr: {}", out.stderr);
}

/// The 15-second case, and the one most likely to be genuinely red:
/// `classify_transport_error` assumes ureq surfaces `io::ErrorKind::TimedOut`.
/// If it reports another kind, this prints `Network error:` and that is a real
/// bug, not a bad test.
#[test]
fn silent_server_times_out_after_fifteen_seconds() {
    let server = LoopbackServer::start();
    server.respond(USAGE_WILDCARD, Response::StaySilent);

    let started = std::time::Instant::now();
    let out = run_usage(&server, "org-1");
    let elapsed = started.elapsed();

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "Request timed out.\n");
    assert!(elapsed.as_secs() >= 14, "timed out far too early: {elapsed:?}");
}

#[test]
fn missing_arguments_print_the_usage_banner() {
    let server = LoopbackServer::start();

    let origin = server.origin();
    let out = run_helper(
        &["usage", "org-1"],
        None,
        &[(BASE_URL_OVERRIDE_VAR, origin.as_str())],
    );

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "Usage: claude-dashboard-helper usage <orgId> <sessionKey>\n");
    assert!(server.recorded().is_empty(), "nothing should reach the network");
}

#[test]
fn org_id_with_whitespace_is_rejected_before_any_request() {
    let server = LoopbackServer::start();

    let out = run_usage(&server, "bad id");

    assert_eq!(out.code, 1);
    assert_eq!(out.stderr, "Invalid orgId.\n");
    assert!(server.recorded().is_empty(), "nothing should reach the network");
}
