//! Starting an account's usage windows by sending one tiny claude.ai message with its
//! session key: create a conversation, complete one prompt, delete it. Port of macOS
//! `apps/macos/Shared/PingService.swift`; the sequence and outcome mapping are
//! `contract/README.md`, "Ping". The completion endpoint is undocumented.

use std::time::Duration;

/// Raw values are stored in `ping_logs.outcome` and are never renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingOutcome {
    Ok = 0,
    Expired = 1,
    Blocked = 2,
    Failed = 3,
}

impl PingOutcome {
    pub fn label(self) -> &'static str {
        match self {
            PingOutcome::Ok => "OK",
            PingOutcome::Expired => "Session expired",
            PingOutcome::Blocked => "Blocked by Cloudflare",
            PingOutcome::Failed => "Failed",
        }
    }

    pub fn from_i64(v: i64) -> Option<Self> {
        match v {
            0 => Some(PingOutcome::Ok),
            1 => Some(PingOutcome::Expired),
            2 => Some(PingOutcome::Blocked),
            3 => Some(PingOutcome::Failed),
            _ => None,
        }
    }
}

pub const PROMPT: &str = "Reply with just: ok";
const NOT_DELETED: &str = "conversation not deleted";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(60);

/// `detail` says why a ping failed, or notes something about an ok one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PingResult {
    pub outcome: PingOutcome,
    pub detail: Option<String>,
}

/// One HTTP reply, whatever its status.
pub struct HttpReply {
    pub status: u16,
    pub cf_mitigated: bool,
    pub body: Vec<u8>,
}

/// Sends one request; `Err` is a transport failure (no reply), with its text.
pub trait PingTransport {
    fn send(
        &self,
        method: &str,
        url: &str,
        body: Option<&serde_json::Value>,
        timeout: Duration,
        session_key: &str,
    ) -> Result<HttpReply, String>;
}

/// Creates conversation `conversation_id` (a lowercase UUID), completes the prompt, then
/// deletes it, best effort and also after a failed completion so no stray chat is left.
pub fn ping_with(
    t: &dyn PingTransport,
    org_id: &str,
    session_key: &str,
    timezone: &str,
    conversation_id: &str,
) -> PingResult {
    let collection = format!("{}/organizations/{org_id}/chat_conversations", crate::api::api_root());
    let create = serde_json::json!({"uuid": conversation_id, "name": ""});
    let created = t.send("POST", &collection, Some(&create), REQUEST_TIMEOUT, session_key);
    if let Some(failure) = failure_of(&created) {
        return failure;
    }

    let complete = serde_json::json!({
        "prompt": PROMPT,
        "timezone": timezone,
        "attachments": [],
        "files": [],
        "rendering_mode": "messages",
    });
    let url = format!("{collection}/{conversation_id}/completion");
    let result = completion_result(&t.send("POST", &url, Some(&complete), COMPLETION_TIMEOUT, session_key));

    let deleted = t.send("DELETE", &format!("{collection}/{conversation_id}"), None, REQUEST_TIMEOUT, session_key);
    if result.outcome == PingOutcome::Ok && failure_of(&deleted).is_some() {
        return PingResult { outcome: PingOutcome::Ok, detail: Some(NOT_DELETED.into()) };
    }
    result
}

/// Sends a real ping through claude.ai (or `CLAUDE_DASHBOARD_API_BASE`).
pub fn ping(org_id: &str, session_key: &str, timezone: &str, conversation_id: &str) -> PingResult {
    ping_with(&UreqTransport, org_id, session_key, timezone, conversation_id)
}

/// `None` when the request succeeded (2xx).
fn failure_of(reply: &Result<HttpReply, String>) -> Option<PingResult> {
    let r = match reply {
        Err(e) => return Some(PingResult { outcome: PingOutcome::Failed, detail: Some(e.clone()) }),
        Ok(r) => r,
    };
    match r.status {
        200..=299 => None,
        401 => Some(PingResult { outcome: PingOutcome::Expired, detail: None }),
        403 => {
            let outcome = if is_cloudflare(r) { PingOutcome::Blocked } else { PingOutcome::Expired };
            Some(PingResult { outcome, detail: None })
        }
        s => Some(PingResult { outcome: PingOutcome::Failed, detail: Some(format!("HTTP {s}")) }),
    }
}

/// A Cloudflare challenge carries `cf-mitigated` or answers with an HTML page; claude.ai's
/// own 403s are JSON.
fn is_cloudflare(r: &HttpReply) -> bool {
    let head = &r.body[..r.body.len().min(64)];
    r.cf_mitigated || String::from_utf8_lossy(head).trim().starts_with('<')
}

/// The SSE reply counts once it carries `message_start`.
fn completion_result(reply: &Result<HttpReply, String>) -> PingResult {
    if let Some(failure) = failure_of(reply) {
        return failure;
    }
    match reply {
        Ok(r) if String::from_utf8_lossy(&r.body).contains("event: message_start") => {
            PingResult { outcome: PingOutcome::Ok, detail: None }
        }
        _ => PingResult { outcome: PingOutcome::Failed, detail: Some("no reply".into()) },
    }
}

/// `ureq` with the usage fetch's headers and cookie; a non-2xx status is a reply.
struct UreqTransport;

impl PingTransport for UreqTransport {
    fn send(
        &self,
        method: &str,
        url: &str,
        body: Option<&serde_json::Value>,
        timeout: Duration,
        session_key: &str,
    ) -> Result<HttpReply, String> {
        let mut req = ureq::request(method, url).timeout(timeout);
        for (name, value) in crate::api::REQUEST_HEADERS {
            req = req.set(name, value);
        }
        req = req.set("Cookie", &format!("sessionKey={session_key}"));
        let sent = match body {
            Some(b) => req.send_string(&b.to_string()),
            None => req.call(),
        };
        let resp = match sent {
            Ok(resp) | Err(ureq::Error::Status(_, resp)) => resp,
            Err(ureq::Error::Transport(t)) => return Err(crate::api::classify_transport_error(t).to_string()),
        };
        let status = resp.status();
        let cf_mitigated = resp.header("cf-mitigated").is_some();
        let mut body = Vec::new();
        std::io::Read::read_to_end(&mut resp.into_reader(), &mut body).map_err(|e| e.to_string())?;
        Ok(HttpReply { status, cf_mitigated, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    type Call = (String, String, Option<serde_json::Value>, Duration);

    struct Fake {
        replies: RefCell<VecDeque<Result<HttpReply, String>>>,
        calls: RefCell<Vec<Call>>,
    }

    fn reply(status: u16, body: &str) -> Result<HttpReply, String> {
        Ok(HttpReply { status, cf_mitigated: false, body: body.as_bytes().to_vec() })
    }

    fn fake(replies: Vec<Result<HttpReply, String>>) -> Fake {
        Fake { replies: RefCell::new(replies.into()), calls: RefCell::new(Vec::new()) }
    }

    impl PingTransport for Fake {
        fn send(
            &self,
            method: &str,
            url: &str,
            body: Option<&serde_json::Value>,
            timeout: Duration,
            session_key: &str,
        ) -> Result<HttpReply, String> {
            assert_eq!(session_key, "sk");
            self.calls.borrow_mut().push((method.into(), url.into(), body.cloned(), timeout));
            self.replies.borrow_mut().pop_front().expect("unexpected request")
        }
    }

    const STREAM: &str = "event: message_start\ndata: {}\n\nevent: message_stop\ndata: {}\n";

    fn run(f: &Fake) -> PingResult {
        ping_with(f, "o1", "sk", "Asia/Ho_Chi_Minh", "c1")
    }

    #[test]
    fn sequence_create_complete_delete() {
        let f = fake(vec![reply(201, "{}"), reply(200, STREAM), reply(204, "")]);
        assert_eq!(run(&f), PingResult { outcome: PingOutcome::Ok, detail: None });
        let calls = f.calls.borrow();
        let root = format!("{}/organizations/o1/chat_conversations", crate::api::api_root());
        assert_eq!(calls.len(), 3);
        assert_eq!((calls[0].0.as_str(), calls[0].1.as_str()), ("POST", root.as_str()));
        assert_eq!(calls[0].2, Some(serde_json::json!({"uuid": "c1", "name": ""})));
        assert_eq!(calls[0].3, Duration::from_secs(30));
        assert_eq!(calls[1].0, "POST");
        assert_eq!(calls[1].1, format!("{root}/c1/completion"));
        assert_eq!(
            calls[1].2,
            Some(serde_json::json!({
                "prompt": "Reply with just: ok",
                "timezone": "Asia/Ho_Chi_Minh",
                "attachments": [],
                "files": [],
                "rendering_mode": "messages",
            }))
        );
        assert_eq!(calls[1].3, Duration::from_secs(60));
        assert_eq!((calls[2].0.as_str(), calls[2].1.clone()), ("DELETE", format!("{root}/c1")));
        assert_eq!(calls[2].2, None);
        assert_eq!(calls[2].3, Duration::from_secs(30));
    }

    #[test]
    fn failed_delete_keeps_ok_with_note() {
        let f = fake(vec![reply(201, "{}"), reply(200, STREAM), reply(500, "")]);
        assert_eq!(
            run(&f),
            PingResult { outcome: PingOutcome::Ok, detail: Some("conversation not deleted".into()) }
        );
    }

    #[test]
    fn unauthorized_is_expired() {
        let f = fake(vec![reply(401, "{}")]);
        assert_eq!(run(&f), PingResult { outcome: PingOutcome::Expired, detail: None });
        assert_eq!(f.calls.borrow().len(), 1);
    }

    #[test]
    fn forbidden_json_is_expired() {
        let f = fake(vec![reply(403, r#"{"error":"x"}"#)]);
        assert_eq!(run(&f).outcome, PingOutcome::Expired);
    }

    #[test]
    fn cloudflare_is_blocked() {
        let f = fake(vec![Ok(HttpReply { status: 403, cf_mitigated: true, body: b"{}".to_vec() })]);
        assert_eq!(run(&f).outcome, PingOutcome::Blocked);
        let f = fake(vec![reply(403, "  <!DOCTYPE html><html>")]);
        assert_eq!(run(&f), PingResult { outcome: PingOutcome::Blocked, detail: None });
    }

    #[test]
    fn rate_limited_is_failed() {
        let f = fake(vec![reply(201, "{}"), reply(429, ""), reply(204, "")]);
        assert_eq!(run(&f), PingResult { outcome: PingOutcome::Failed, detail: Some("HTTP 429".into()) });
        assert_eq!(f.calls.borrow()[2].0, "DELETE");
    }

    #[test]
    fn no_message_start_is_failed() {
        let f = fake(vec![reply(201, "{}"), reply(200, "event: error\ndata: {}\n"), reply(204, "")]);
        assert_eq!(run(&f), PingResult { outcome: PingOutcome::Failed, detail: Some("no reply".into()) });
        assert_eq!(f.calls.borrow().len(), 3);
    }

    #[test]
    fn transport_error_is_failed() {
        let f = fake(vec![Err("timed out".into())]);
        assert_eq!(run(&f), PingResult { outcome: PingOutcome::Failed, detail: Some("timed out".into()) });
    }

    #[test]
    fn labels_and_raw_values() {
        assert_eq!(PingOutcome::Ok.label(), "OK");
        assert_eq!(PingOutcome::Expired.label(), "Session expired");
        assert_eq!(PingOutcome::Blocked.label(), "Blocked by Cloudflare");
        assert_eq!(PingOutcome::Failed.label(), "Failed");
        assert_eq!(PingOutcome::from_i64(2), Some(PingOutcome::Blocked));
        assert_eq!(PingOutcome::from_i64(9), None);
    }
}
