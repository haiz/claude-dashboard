//! `claude-dashboard-bridge`: one native-messaging message in, one reply out,
//! then exit. The browser launches a fresh host per message.

use std::io::{stdin, stdout};

use claude_dashboard_bridge::framing::{read_message, write_message, FramingError};
use claude_dashboard_bridge::handler::handle;
use claude_dashboard_bridge::real_env::RealEnv;

fn main() {
    let raw = match read_message(&mut stdin().lock()) {
        Ok(raw) => raw,
        // Browser disconnected before sending: nothing to do.
        Err(FramingError::Eof) => return,
        // Any other framing failure leaves nothing to reply to.
        Err(_) => std::process::exit(1),
    };
    let reply = handle(&raw, &RealEnv);
    let body = serde_json::to_vec(&reply).expect("Reply serializes");
    if write_message(&mut stdout().lock(), &body).is_err() {
        std::process::exit(1);
    }
}
