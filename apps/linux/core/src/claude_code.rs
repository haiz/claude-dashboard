//! Ports ClaudeCodeAccountDetector.swift: the email Claude Code is signed in
//! as, from `~/.claude.json` (`%USERPROFILE%\.claude.json` on Windows).

use std::path::PathBuf;

pub fn active_email_from(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let email = v.get("oauthAccount")?.get("emailAddress")?.as_str()?.trim();
    (!email.is_empty()).then(|| email.to_string())
}

pub fn claude_json_path() -> PathBuf {
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    match std::env::var(home_var) {
        Ok(h) if !h.is_empty() => PathBuf::from(h).join(".claude.json"),
        _ => PathBuf::from(".claude.json"),
    }
}

pub fn active_email() -> Option<String> {
    std::fs::read_to_string(claude_json_path()).ok().and_then(|t| active_email_from(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_oauth_email() {
        let t = r#"{"oauthAccount":{"emailAddress":"  me@x.com "},"other":1}"#;
        assert_eq!(active_email_from(t).as_deref(), Some("me@x.com"));
    }

    #[test]
    fn missing_or_bad_yields_none() {
        for t in ["", "{", "{}", r#"{"oauthAccount":{}}"#, r#"{"oauthAccount":{"emailAddress":""}}"#,
                  r#"{"oauthAccount":{"emailAddress":42}}"#, r#"{"oauthAccount":null}"#] {
            assert_eq!(active_email_from(t), None, "{t:?}");
        }
    }

    #[test]
    fn path_is_dot_claude_json() {
        assert_eq!(claude_json_path().file_name().unwrap(), ".claude.json");
    }
}
