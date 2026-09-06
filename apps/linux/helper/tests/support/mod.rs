//! Raw-TCP loopback server and process spawner for the helper's transport
//! tests. Mirrors `apps/macos/HelperTests/LoopbackServer.swift` and
//! `HelperProcess.swift` deliberately: the same scripting model on both
//! platforms means a contract change is the same edit twice.
//!
//! Raw sockets because the failure modes are the point — a connection closed
//! mid-request, a server that stays silent past the 15-second timeout, and a
//! body that is not valid UTF-8.

#![allow(dead_code)] // each integration test binary uses a subset

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

pub const USAGE_WILDCARD: &str = "/api/organizations/*/usage";

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    /// Header names lowercased; HTTP header names are case-insensitive.
    pub headers: HashMap<String, String>,
}

#[derive(Clone)]
pub enum Response {
    Reply {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    CloseImmediately,
    StaySilent,
}

impl Response {
    pub fn json(body: &str) -> Response {
        Response::Reply {
            status: 200,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn status(status: u16) -> Response {
        Response::Reply {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }
}

#[derive(Default)]
struct State {
    routes: HashMap<String, Response>,
    records: Vec<RecordedRequest>,
    /// Connections parked by `StaySilent`. NOT dropped when `LoopbackServer`
    /// is: the accept-loop thread holds its own clone of this `Arc` and never
    /// exits (`for stream in listener.incoming()` runs until the process
    /// does), so these sockets — and the thread, and the bound listener
    /// port — live until the test process itself exits. Harmless: each
    /// `start()` binds an OS-assigned port that stays reserved for the
    /// process's lifetime, so a later server can never reuse a port an
    /// earlier test's leaked thread is still listening on.
    held: Vec<TcpStream>,
}

pub struct LoopbackServer {
    port: u16,
    state: Arc<Mutex<State>>,
}

impl LoopbackServer {
    pub fn start() -> LoopbackServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local_addr").port();
        let state = Arc::new(Mutex::new(State::default()));
        let thread_state = Arc::clone(&state);

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                handle(stream, &thread_state);
            }
        });

        LoopbackServer { port, state }
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn respond(&self, path: &str, response: Response) {
        self.state.lock().unwrap().routes.insert(path.to_string(), response);
    }

    pub fn recorded(&self) -> Vec<RecordedRequest> {
        self.state.lock().unwrap().records.clone()
    }
}

fn matches_usage_wildcard(path: &str) -> bool {
    path.starts_with("/api/organizations/") && path.ends_with("/usage")
}

fn handle(mut stream: TcpStream, state: &Arc<Mutex<State>>) {
    let Some(request) = read_request(&mut stream) else { return };

    let response = {
        let mut guard = state.lock().unwrap();
        guard.records.push(request.clone());
        // Cloned eagerly rather than chained with `or_else`: a closure that
        // borrows `guard` while `get` already borrows it does not compile.
        let exact = guard.routes.get(&request.path).cloned();
        match exact {
            Some(found) => Some(found),
            None if matches_usage_wildcard(&request.path) => {
                guard.routes.get(USAGE_WILDCARD).cloned()
            }
            None => None,
        }
    };

    match response {
        // A path nobody scripted is a test-authoring error: answer 404 so the
        // test fails on `HTTP 404` rather than hanging.
        None => write_response(&mut stream, 404, &[], &[]),
        Some(Response::Reply { status, headers, body }) => {
            write_response(&mut stream, status, &headers, &body)
        }
        Some(Response::CloseImmediately) => drop(stream),
        Some(Response::StaySilent) => {
            // Park the connection: closing it would surface as a network error
            // instead of the client's own 15-second timeout.
            state.lock().unwrap().held.push(stream);
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<RecordedRequest> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let text = String::from_utf8_lossy(&raw).to_string();
    let mut lines = text.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Some(RecordedRequest { method, path, headers })
}

fn write_response(stream: &mut TcpStream, status: u16, headers: &[(String, String)], body: &[u8]) {
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

// ---------------------------------------------------------------------
// Process spawning
// ---------------------------------------------------------------------

pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub code: i32,
}

impl Output {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }
}

/// Runs the real helper binary. stdin is written **and closed** before waiting:
/// `run_add_key` blocks on `read_to_string`, so leaving the pipe open hangs.
pub fn run_helper(args: &[&str], stdin: Option<&str>, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_claude-dashboard-helper"));
    command.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn helper");
    {
        let mut pipe = child.stdin.take().expect("stdin");
        if let Some(text) = stdin {
            pipe.write_all(text.as_bytes()).expect("write stdin");
        }
        // `pipe` is dropped here, closing the descriptor.
    }
    let out = child.wait_with_output().expect("wait helper");
    Output {
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}
