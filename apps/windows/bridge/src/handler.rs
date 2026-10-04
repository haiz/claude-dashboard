//! One extension message in, one reply out. Order matters:
//! 1. parse and validate the message (no I/O);
//! 2. muted check (cheap, before the network);
//! 3. network: `/api/account`, then `/api/organizations` — never under the lock;
//! 4. under `lock()`: reload sources, re-check muted, load accounts,
//!    `apply_session_key`, save accounts, bind the install, save sources;
//! 5. after the lock: notify the app.
//!
//! The session key never appears in a reply, on any branch.

use claude_dashboard_core::api::ParsedAccount;
use claude_dashboard_core::key_intake::{apply_session_key, IntakeOutcome};
use claude_dashboard_core::manual_key::trimmed_key;
use claude_dashboard_core::model::Account;
use claude_dashboard_core::plan::ParsedOrg;
use serde::{Deserialize, Serialize};

use crate::sources::ExtensionSources;

/// The side effects `handle` needs, injected so tests touch no network, store
/// or pipe. `RealEnv` (Task 10) is the production implementation.
pub trait Environment {
    /// Held across the store read-modify-write; dropped before `notify_app`.
    type Guard;

    fn fetch_account(&self, session_key: &str) -> Option<ParsedAccount>;
    fn fetch_orgs(&self, session_key: &str) -> Vec<ParsedOrg>;
    fn lock(&self) -> Result<Self::Guard, String>;
    fn load_accounts(&self) -> Result<Vec<Account>, String>;
    fn save_accounts(&self, accounts: &[Account]) -> Result<(), String>;
    fn load_sources(&self) -> Result<ExtensionSources, String>;
    fn save_sources(&self, sources: &ExtensionSources) -> Result<(), String>;
    fn new_id(&self) -> String;
    fn now_reference(&self) -> f64;
    fn notify_app(&self);
}

#[derive(Deserialize)]
struct Incoming {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, rename = "installId")]
    install_id: String,
    #[serde(default)]
    browser: String,
    #[serde(default, rename = "sessionKey")]
    session_key: String,
}

/// The reply written back to the extension. Omitted fields are absent in JSON.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Reply {
    fn ok(email: Option<String>) -> Self {
        Self {
            ok: true,
            email,
            error: None,
            message: None,
        }
    }

    fn err(code: &str, msg: &str) -> Self {
        Self {
            ok: false,
            email: None,
            error: Some(code.into()),
            message: Some(msg.into()),
        }
    }
}

/// Processes one raw message body. See the module docs for the ordered steps
/// and the error codes (`bad_message`, `no_key`, `muted`, `rejected`,
/// `no_chat_org`, `store`).
pub fn handle(raw: &[u8], env: &impl Environment) -> Reply {
    let Ok(msg) = serde_json::from_slice::<Incoming>(raw) else {
        return Reply::err("bad_message", "Could not parse the message.");
    };
    if msg.kind != "sessionKey" {
        return Reply::err("bad_message", "Unknown message type.");
    }
    let Some(session_key) = trimmed_key(&msg.session_key) else {
        return Reply::err("no_key", "No session key in the message.");
    };
    if msg.install_id.is_empty() {
        return Reply::err("bad_message", "No install id.");
    }

    // Muted is checked before the network so a deleted account's extension
    // cannot even cause a fetch. Re-checked again under the lock.
    match env.load_sources() {
        Ok(s) if s.is_muted(&msg.install_id) => {
            return Reply::err("muted", "This account was removed.")
        }
        Ok(_) => {}
        Err(e) => return Reply::err("store", &e),
    }

    let Some(identity) = env.fetch_account(session_key) else {
        return Reply::err("rejected", "The session key was not accepted.");
    };
    let orgs = env.fetch_orgs(session_key);

    let guard = match env.lock() {
        Ok(g) => g,
        Err(e) => return Reply::err("store", &e),
    };

    // Re-read under the lock: a mute (or any change) may have landed between
    // the pre-network check and here.
    let mut sources = match env.load_sources() {
        Ok(s) => s,
        Err(e) => return Reply::err("store", &e),
    };
    if sources.is_muted(&msg.install_id) {
        return Reply::err("muted", "This account was removed.");
    }
    let mut accounts = match env.load_accounts() {
        Ok(a) => a,
        Err(e) => return Reply::err("store", &e),
    };

    let outcome = apply_session_key(
        &mut accounts,
        session_key,
        &identity,
        &orgs,
        || env.new_id(),
        env.now_reference(),
    );
    let account_id = match &outcome {
        IntakeOutcome::RejectNoChatOrg => {
            return Reply::err("no_chat_org", "No organization with chat access.");
        }
        IntakeOutcome::Added { account_id, .. } | IntakeOutcome::Updated { account_id, .. } => {
            account_id.clone()
        }
    };

    if let Err(e) = env.save_accounts(&accounts) {
        return Reply::err("store", &e);
    }
    sources.bind(&msg.install_id, &account_id, &msg.browser);
    if let Err(e) = env.save_sources(&sources) {
        return Reply::err("store", &e);
    }
    drop(guard);

    env.notify_app();
    Reply::ok(identity.email)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_dashboard_core::api::parse_account;
    use claude_dashboard_core::model::AccountStatus;
    use claude_dashboard_core::plan::parse_orgs;
    use std::cell::{Cell, RefCell};

    struct FakeEnv {
        identity: Option<ParsedAccount>,
        orgs_body: String,
        accounts: RefCell<Vec<Account>>,
        sources: RefCell<ExtensionSources>,
        fetches: Cell<u32>,
        account_saves: Cell<u32>,
        notified: Cell<u32>,
        fail_account_save: bool,
    }

    impl FakeEnv {
        fn new(identity: Option<ParsedAccount>, orgs_body: &str) -> Self {
            Self {
                identity,
                orgs_body: orgs_body.to_string(),
                accounts: RefCell::new(Vec::new()),
                sources: RefCell::new(ExtensionSources::default()),
                fetches: Cell::new(0),
                account_saves: Cell::new(0),
                notified: Cell::new(0),
                fail_account_save: false,
            }
        }
    }

    impl Environment for FakeEnv {
        type Guard = ();

        fn fetch_account(&self, _session_key: &str) -> Option<ParsedAccount> {
            self.fetches.set(self.fetches.get() + 1);
            self.identity.clone()
        }
        fn fetch_orgs(&self, _session_key: &str) -> Vec<ParsedOrg> {
            parse_orgs(&self.orgs_body)
        }
        fn lock(&self) -> Result<(), String> {
            Ok(())
        }
        fn load_accounts(&self) -> Result<Vec<Account>, String> {
            Ok(self.accounts.borrow().clone())
        }
        fn save_accounts(&self, accounts: &[Account]) -> Result<(), String> {
            if self.fail_account_save {
                return Err("disk full".into());
            }
            self.account_saves.set(self.account_saves.get() + 1);
            *self.accounts.borrow_mut() = accounts.to_vec();
            Ok(())
        }
        fn load_sources(&self) -> Result<ExtensionSources, String> {
            Ok(self.sources.borrow().clone())
        }
        fn save_sources(&self, sources: &ExtensionSources) -> Result<(), String> {
            *self.sources.borrow_mut() = sources.clone();
            Ok(())
        }
        fn new_id(&self) -> String {
            "NEW-ID".into()
        }
        fn now_reference(&self) -> f64 {
            0.0
        }
        fn notify_app(&self) {
            self.notified.set(self.notified.get() + 1);
        }
    }

    fn identity(uuid: &str, email: Option<&str>, caps: &str) -> ParsedAccount {
        let email_field = email.map(|e| format!(r#""email_address":"{e}","#)).unwrap_or_default();
        parse_account(&format!(
            r#"{{{email_field}"uuid":"{uuid}","memberships":[
                {{"organization":{{"uuid":"org-1","name":"Org","capabilities":{caps}}}}}]}}"#
        ))
        .unwrap()
    }

    fn message(install: &str, browser: &str, key: &str) -> Vec<u8> {
        serde_json::json!({
            "type": "sessionKey",
            "installId": install,
            "browser": browser,
            "sessionKey": key,
        })
        .to_string()
        .into_bytes()
    }

    fn stored_manual(uuid: &str, email: &str) -> Account {
        Account::from_json_object(&format!(
            r#"{{"id":"OLD-ID","name":"{email}","email":"{email}","accountUuid":"{uuid}",
                "chromeProfilePath":"","orgId":"org-1","plan":"Pro","status":"expired",
                "source":"manual"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn valid_message_adds_binds_and_notifies() {
        let env = FakeEnv::new(
            Some(identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#)),
            r#"[{"uuid":"org-1","name":"Org","capabilities":["chat","claude_max"]}]"#,
        );
        let reply = handle(&message("inst-1", "edge", "sk-key"), &env);

        assert_eq!(reply, Reply::ok(Some("a@x.com".into())));
        assert_eq!(env.accounts.borrow().len(), 1);
        assert_eq!(env.accounts.borrow()[0].email.as_deref(), Some("a@x.com"));
        assert_eq!(env.sources.borrow().bindings["inst-1"].account_id, "NEW-ID");
        assert_eq!(env.sources.borrow().bindings["inst-1"].browser, "edge");
        assert_eq!(env.notified.get(), 1);
    }

    #[test]
    fn known_identity_is_repaired_not_duplicated() {
        let env = FakeEnv::new(
            Some(identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#)),
            r#"[{"uuid":"org-1","name":"Org","capabilities":["chat","claude_max"]}]"#,
        );
        env.accounts.borrow_mut().push(stored_manual("acct-1", "a@x.com"));

        let reply = handle(&message("inst-1", "chrome", "sk-fresh"), &env);

        assert!(reply.ok);
        assert_eq!(env.accounts.borrow().len(), 1, "repaired in place, not duplicated");
        assert_eq!(env.accounts.borrow()[0].id, "OLD-ID");
        assert_eq!(env.accounts.borrow()[0].status, AccountStatus::Active);
        assert_eq!(env.notified.get(), 1);
    }

    #[test]
    fn muted_install_writes_nothing() {
        let env = FakeEnv::new(
            Some(identity("acct-1", Some("a@x.com"), r#"["chat"]"#)),
            r#"[{"uuid":"org-1","name":"Org","capabilities":["chat"]}]"#,
        );
        env.sources.borrow_mut().muted.insert("inst-1".into());

        let reply = handle(&message("inst-1", "chrome", "sk-key"), &env);

        assert_eq!(reply.error.as_deref(), Some("muted"));
        assert!(env.accounts.borrow().is_empty());
        assert_eq!(env.account_saves.get(), 0);
        assert_eq!(env.fetches.get(), 0, "muted is checked before the network");
        assert_eq!(env.notified.get(), 0);
    }

    #[test]
    fn non_json_is_bad_message() {
        let env = FakeEnv::new(None, "[]");
        let reply = handle(b"{ not json", &env);
        assert_eq!(reply.error.as_deref(), Some("bad_message"));
        assert_eq!(env.fetches.get(), 0);
    }

    #[test]
    fn missing_fields_are_rejected_before_network() {
        let env = FakeEnv::new(Some(identity("acct-1", Some("a@x.com"), r#"["chat"]"#)), "[]");

        let wrong_type = serde_json::json!({"type":"hello","installId":"i","sessionKey":"sk"})
            .to_string()
            .into_bytes();
        assert_eq!(handle(&wrong_type, &env).error.as_deref(), Some("bad_message"));

        assert_eq!(handle(&message("i", "chrome", "   "), &env).error.as_deref(), Some("no_key"));
        assert_eq!(handle(&message("", "chrome", "sk"), &env).error.as_deref(), Some("bad_message"));

        assert_eq!(env.fetches.get(), 0);
    }

    #[test]
    fn unaccepted_key_replies_rejected() {
        let env = FakeEnv::new(None, "[]");
        let reply = handle(&message("inst-1", "chrome", "sk-bad"), &env);
        assert_eq!(reply.error.as_deref(), Some("rejected"));
        assert!(env.accounts.borrow().is_empty());
        assert_eq!(env.notified.get(), 0);
    }

    #[test]
    fn no_chat_org_is_reported() {
        let env = FakeEnv::new(Some(identity("acct-1", Some("a@x.com"), r#"["api"]"#)), "[]");
        let reply = handle(&message("inst-1", "chrome", "sk-key"), &env);
        assert_eq!(reply.error.as_deref(), Some("no_chat_org"));
        assert!(env.accounts.borrow().is_empty());
        assert_eq!(env.notified.get(), 0);
    }

    #[test]
    fn a_store_failure_is_reported_and_does_not_notify() {
        let mut env = FakeEnv::new(
            Some(identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#)),
            r#"[{"uuid":"org-1","name":"Org","capabilities":["chat","claude_max"]}]"#,
        );
        env.fail_account_save = true;

        let reply = handle(&message("inst-1", "chrome", "sk-key"), &env);

        assert_eq!(reply.error.as_deref(), Some("store"));
        assert_eq!(env.notified.get(), 0);
        assert!(env.sources.borrow().bindings.is_empty(), "no bind if the account save failed");
    }

    #[test]
    fn the_reply_never_echoes_the_key() {
        let env = FakeEnv::new(
            Some(identity("acct-1", Some("a@x.com"), r#"["chat","claude_max"]"#)),
            r#"[{"uuid":"org-1","name":"Org","capabilities":["chat","claude_max"]}]"#,
        );
        let reply = handle(&message("inst-1", "chrome", "sk-secret-value"), &env);
        let json = serde_json::to_string(&reply).unwrap();
        assert!(!json.contains("sk-secret-value"), "reply leaked the key: {json}");
    }
}
