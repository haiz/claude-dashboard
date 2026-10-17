//! First Claude Code login for an account with no usable vault copy (port of macOS
//! `ClaudeCodeLoginProvisioner`, `OAuthCallbackListener`, `BrowserProfileOpener` and the
//! network half of `ClaudeCodeOAuthClient`): open the account's browser profile on Claude's
//! consent page, catch the redirect on a loopback listener, exchange the code, and check the
//! email before anything is stored.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use claude_dashboard_core::cc_oauth::{self, Pkce};
use claude_dashboard_core::cc_switch::model::{email_of, VaultEntry};
use claude_dashboard_core::model::Browser;

/// How long the browser has to come back (macOS `browserTimeout`).
pub const BROWSER_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq)]
pub struct ProvisionInput {
    pub email: String,
    pub browser: Browser,
    /// The profile directory (`Profile 1`); `None` for an extension-fed account, whose URL
    /// opens in its browser without a profile switch.
    pub profile_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionError {
    Cancelled,
    TimedOut,
    ListenerFailed,
    /// The browser signed in as this other email.
    EmailMismatch(String),
    Failed,
}

impl ProvisionError {
    /// The message after a failed sign-in (macOS `provisionMessage`); `None` when cancelled.
    pub fn message(&self, name: &str) -> Option<String> {
        Some(match self {
            ProvisionError::Cancelled => return None,
            ProvisionError::TimedOut => format!("Timed out waiting for the browser. Try Switch again for {name}."),
            ProvisionError::ListenerFailed => format!("Could not open a local callback for {name}. Try again."),
            ProvisionError::EmailMismatch(got) => format!("The browser signed in as {got}, not {name}. Nothing was changed."),
            ProvisionError::Failed => format!(
                "Could not sign {name} in to Claude Code. Make sure that browser profile is logged in to claude.ai as {name}, then try Switch again."
            ),
        })
    }
}

/// A one-shot loopback HTTP server for the OAuth redirect on `127.0.0.1`, OS-chosen port.
pub struct Listener {
    inner: TcpListener,
}

impl Listener {
    pub fn start() -> std::io::Result<(Listener, u16)> {
        let inner = TcpListener::bind(("127.0.0.1", 0))?;
        inner.set_nonblocking(true)?;
        let port = inner.local_addr()?.port();
        Ok((Listener { inner }, port))
    }

    /// Waits for `/callback?code=...&state=<expected_state>`. Every request gets a small page;
    /// one with another state is ignored, so a stray hit cannot resolve the wait.
    pub fn wait(&self, expected_state: &str, timeout: Duration, cancel: &AtomicBool) -> Result<String, ProvisionError> {
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(ProvisionError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(ProvisionError::TimedOut);
            }
            match self.inner.accept() {
                Ok((stream, _)) => {
                    if let Some(code) = serve(stream, expected_state) {
                        return Ok(code);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => return Err(ProvisionError::ListenerFailed),
            }
        }
    }
}

/// Reads one request, answers it with a small page, and returns its code when it is our callback.
fn serve(mut stream: TcpStream, expected_state: &str) -> Option<String> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = vec![0u8; 64 * 1024];
    let n = stream.read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]);
    let code = cc_oauth::callback_code(&request, expected_state);
    let (title, body) = if code.is_some() {
        ("Signed in", "Claude Code is now linked. You can close this tab and return to Claude Dashboard.")
    } else {
        ("Waiting", "Claude Dashboard is still waiting. You can close this tab.")
    };
    let html = format!(
        "<!doctype html><meta charset=utf-8><title>Claude Dashboard</title>\
         <body style=\"font:16px 'Segoe UI',sans-serif;margin:4rem auto;max-width:28rem;text-align:center\">\
         <h2>{title}</h2><p>{body}</p></body>"
    );
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.flush();
    code
}

/// The exe and arguments that open `url` in `browser` (profile `profile_dir` when given),
/// looking under each of `roots` (`%ProgramFiles%`, `%ProgramFiles(x86)%`, `%LOCALAPPDATA%`).
/// `None` when the browser is not installed there (or has no Windows build).
pub fn browser_command(browser: &Browser, profile_dir: Option<&str>, url: &str, roots: &[PathBuf]) -> Option<(PathBuf, Vec<String>)> {
    let rel: &[&str] = match browser {
        Browser::Chrome => &["Google", "Chrome", "Application", "chrome.exe"],
        Browser::ChromeDev => &["Google", "Chrome Dev", "Application", "chrome.exe"],
        Browser::Edge => &["Microsoft", "Edge", "Application", "msedge.exe"],
        Browser::Brave => &["BraveSoftware", "Brave-Browser", "Application", "brave.exe"],
        Browser::Arc => return None,
    };
    let exe = roots.iter().map(|r| rel.iter().fold(r.clone(), |p, part| p.join(part))).find(|p| p.is_file())?;
    let mut args: Vec<String> = profile_dir.map(|d| format!("--profile-directory={d}")).into_iter().collect();
    args.push(url.to_string());
    Some((exe, args))
}

/// `%ProgramFiles%`, `%ProgramFiles(x86)%` and `%LOCALAPPDATA%`, where browsers install.
pub fn install_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .collect()
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The token exchange, then the profile call that supplies the plan and `oauthAccount`.
pub fn exchange(code: &str, pkce: &Pkce, redirect_uri: &str) -> Result<VaultEntry, ProvisionError> {
    let fail = |e: ureq::Error| {
        eprintln!("[cc-switch] token exchange failed: {}", e.kind());
        ProvisionError::Failed
    };
    let token: serde_json::Value = ureq::post(cc_oauth::TOKEN_URL)
        .timeout(Duration::from_secs(30))
        .set("Content-Type", "application/json")
        .send_string(&cc_oauth::token_body(code, pkce, redirect_uri).to_string())
        .map_err(fail)?
        .into_string()
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or(ProvisionError::Failed)?;
    let access = token.get("access_token").and_then(|v| v.as_str()).ok_or(ProvisionError::Failed)?;
    let profile: serde_json::Value = ureq::get(cc_oauth::PROFILE_URL)
        .timeout(Duration::from_secs(30))
        .set("Authorization", &format!("Bearer {access}"))
        .set("Content-Type", "application/json")
        .set("Cache-Control", "no-cache")
        .call()
        .map_err(fail)?
        .into_string()
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or(ProvisionError::Failed)?;
    cc_oauth::entry_from(&token, &profile, unix_ms()).ok_or(ProvisionError::Failed)
}

/// The real browser and endpoints.
pub struct RealDeps;

impl ProvisionDeps for RealDeps {
    fn open(&self, input: &ProvisionInput, url: &str) -> bool {
        let Some((exe, args)) = browser_command(&input.browser, input.profile_dir.as_deref(), url, &install_roots()) else {
            eprintln!("[cc-switch] {} is not installed in a known place", input.browser.display_name());
            return false;
        };
        std::process::Command::new(exe).args(args).spawn().is_ok()
    }

    fn exchange(&self, code: &str, pkce: &Pkce, redirect_uri: &str) -> Result<VaultEntry, ProvisionError> {
        exchange(code, pkce, redirect_uri)
    }
}

/// What `provision` needs from the outside world; the real one opens the browser and calls
/// the token and profile endpoints.
pub trait ProvisionDeps {
    fn open(&self, input: &ProvisionInput, url: &str) -> bool;
    fn exchange(&self, code: &str, pkce: &Pkce, redirect_uri: &str) -> Result<VaultEntry, ProvisionError>;
}

/// Mints a Claude Code login for `input`. `on_awaiting` fires once the browser is open;
/// setting `cancel` ends the wait with `Cancelled`. Blocks: call from a worker thread.
pub fn provision(
    input: &ProvisionInput,
    cancel: &AtomicBool,
    on_awaiting: impl FnOnce(),
    deps: &dyn ProvisionDeps,
    timeout: Duration,
) -> Result<VaultEntry, ProvisionError> {
    let pkce = Pkce::make();
    let (listener, port) = Listener::start().map_err(|_| ProvisionError::ListenerFailed)?;
    // `localhost`, not `127.0.0.1`: the redirect Claude Code's client registers; the token
    // endpoint matches redirect_uri as an exact string.
    let redirect_uri = format!("http://localhost:{port}/callback");
    let url = cc_oauth::authorize_url(&pkce, &redirect_uri, Some(&input.email));
    if !deps.open(input, &url) {
        return Err(ProvisionError::Failed);
    }
    on_awaiting();
    let code = listener.wait(&pkce.state, timeout, cancel)?;
    // A Cancel that lands while the code is exchanged still wins: nothing is stored or switched.
    let cancelled = || cancel.load(Ordering::SeqCst);
    if cancelled() {
        return Err(ProvisionError::Cancelled);
    }
    let entry = deps.exchange(&code, &pkce, &redirect_uri)?;
    if cancelled() {
        return Err(ProvisionError::Cancelled);
    }
    let got = email_of(&entry.oauth_account).unwrap_or_default();
    if !got.eq_ignore_ascii_case(&input.email) {
        return Err(ProvisionError::EmailMismatch(got));
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn get(port: u16, path: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n").unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn listener_returns_code_for_matching_state() {
        let (l, port) = Listener::start().unwrap();
        let t = std::thread::spawn(move || {
            let stray = get(port, "/callback?code=x&state=wrong");
            let page = get(port, "/callback?code=abc&state=st");
            (stray, page)
        });
        let code = l.wait("st", Duration::from_secs(10), &AtomicBool::new(false));
        let (stray, page) = t.join().unwrap();
        assert_eq!(code, Ok("abc".into()));
        assert!(stray.starts_with("HTTP/1.1 200") && stray.contains("still waiting"));
        assert!(page.contains("Claude Code is now linked"));
    }

    #[test]
    fn listener_cancel_and_timeout() {
        let (l, _) = Listener::start().unwrap();
        assert_eq!(l.wait("st", Duration::from_millis(200), &AtomicBool::new(false)), Err(ProvisionError::TimedOut));
        assert_eq!(l.wait("st", Duration::from_secs(10), &AtomicBool::new(true)), Err(ProvisionError::Cancelled));
    }

    fn roots_with(exe: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(exe.join("\\"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"").unwrap();
        let roots = vec![d.path().join("missing"), d.path().to_path_buf()];
        (d, roots)
    }

    #[test]
    fn browser_command_uses_profile_directory() {
        let (d, roots) = roots_with(&["Google", "Chrome", "Application", "chrome.exe"]);
        let (exe, args) = browser_command(&Browser::Chrome, Some("Profile 1"), "https://u", &roots).unwrap();
        assert_eq!(exe, d.path().join("Google").join("Chrome").join("Application").join("chrome.exe"));
        assert_eq!(args, vec!["--profile-directory=Profile 1".to_string(), "https://u".to_string()]);
        let (_d, roots) = roots_with(&["Microsoft", "Edge", "Application", "msedge.exe"]);
        assert!(browser_command(&Browser::Edge, Some("Default"), "u", &roots).is_some());
        let (_d, roots) = roots_with(&["BraveSoftware", "Brave-Browser", "Application", "brave.exe"]);
        assert!(browser_command(&Browser::Brave, Some("Default"), "u", &roots).is_some());
        let (_d, roots) = roots_with(&["Google", "Chrome Dev", "Application", "chrome.exe"]);
        assert!(browser_command(&Browser::ChromeDev, Some("Default"), "u", &roots).is_some());
    }

    #[test]
    fn browser_command_without_profile_for_extension_accounts() {
        let (_d, roots) = roots_with(&["Microsoft", "Edge", "Application", "msedge.exe"]);
        let (_, args) = browser_command(&Browser::Edge, None, "https://u", &roots).unwrap();
        assert_eq!(args, vec!["https://u".to_string()]);
    }

    #[test]
    fn browser_command_none_when_not_installed() {
        let d = tempfile::tempdir().unwrap();
        assert!(browser_command(&Browser::Chrome, Some("Default"), "u", &[d.path().to_path_buf()]).is_none());
        assert!(browser_command(&Browser::Arc, None, "u", &[d.path().to_path_buf()]).is_none());
    }

    /// Opens nothing; instead follows the redirect itself with a code, like a browser would.
    struct FakeBrowser {
        email: String,
    }

    impl ProvisionDeps for FakeBrowser {
        fn open(&self, input: &ProvisionInput, url: &str) -> bool {
            assert!(url.contains(&format!("login_hint={}", input.email.replace('@', "%40"))));
            let param = |name: &str| {
                url.split(['?', '&']).find_map(|kv| kv.strip_prefix(&format!("{name}="))).unwrap().to_string()
            };
            let state = param("state");
            let redirect = param("redirect_uri").replace("%3A", ":").replace("%2F", "/");
            let port: u16 = redirect.trim_start_matches("http://localhost:").trim_end_matches("/callback").parse().unwrap();
            std::thread::spawn(move || get(port, &format!("/callback?code=the-code&state={state}")));
            true
        }
        fn exchange(&self, code: &str, _pkce: &Pkce, redirect_uri: &str) -> Result<VaultEntry, ProvisionError> {
            assert_eq!(code, "the-code");
            assert!(redirect_uri.starts_with("http://localhost:") && redirect_uri.ends_with("/callback"));
            let token = json!({"access_token": "at", "refresh_token": "rt", "expires_in": 100});
            let profile = json!({"account": {"email": self.email}, "organization": {}});
            Ok(cc_oauth::entry_from(&token, &profile, 0).unwrap())
        }
    }

    fn input() -> ProvisionInput {
        ProvisionInput { email: "a@x.com".into(), browser: Browser::Chrome, profile_dir: Some("Default".into()) }
    }

    #[test]
    fn provision_happy_path() {
        let mut awaited = false;
        let e = provision(&input(), &AtomicBool::new(false), || awaited = true, &FakeBrowser { email: "A@x.com".into() }, Duration::from_secs(10)).unwrap();
        assert!(awaited);
        assert_eq!(email_of(&e.oauth_account).as_deref(), Some("A@x.com"));
    }

    /// A browser that comes back with a code, but the user cancels while the exchange runs.
    struct CancelDuringExchange {
        inner: FakeBrowser,
        cancel: std::sync::Arc<AtomicBool>,
    }

    impl ProvisionDeps for CancelDuringExchange {
        fn open(&self, input: &ProvisionInput, url: &str) -> bool {
            self.inner.open(input, url)
        }
        fn exchange(&self, code: &str, pkce: &Pkce, redirect_uri: &str) -> Result<VaultEntry, ProvisionError> {
            self.cancel.store(true, Ordering::SeqCst);
            self.inner.exchange(code, pkce, redirect_uri)
        }
    }

    #[test]
    fn cancel_during_exchange_returns_cancelled() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let deps = CancelDuringExchange { inner: FakeBrowser { email: "a@x.com".into() }, cancel: cancel.clone() };
        let r = provision(&input(), &cancel, || {}, &deps, Duration::from_secs(10));
        assert_eq!(r, Err(ProvisionError::Cancelled));
    }

    #[test]
    fn provision_rejects_other_email() {
        let r = provision(&input(), &AtomicBool::new(false), || {}, &FakeBrowser { email: "b@x.com".into() }, Duration::from_secs(10));
        assert_eq!(r, Err(ProvisionError::EmailMismatch("b@x.com".into())));
    }

    #[test]
    fn messages_match_macos() {
        let n = "a@x.com";
        assert_eq!(ProvisionError::Cancelled.message(n), None);
        assert_eq!(ProvisionError::TimedOut.message(n).unwrap(), "Timed out waiting for the browser. Try Switch again for a@x.com.");
        assert_eq!(ProvisionError::ListenerFailed.message(n).unwrap(), "Could not open a local callback for a@x.com. Try again.");
        assert_eq!(ProvisionError::EmailMismatch("b@x.com".into()).message(n).unwrap(), "The browser signed in as b@x.com, not a@x.com. Nothing was changed.");
        assert_eq!(
            ProvisionError::Failed.message(n).unwrap(),
            "Could not sign a@x.com in to Claude Code. Make sure that browser profile is logged in to claude.ai as a@x.com, then try Switch again."
        );
    }
}
