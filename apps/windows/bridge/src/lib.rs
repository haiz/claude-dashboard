//! Native-messaging host for the Claude Dashboard browser extension: receives
//! one `sessionKey` message, stores the account, replies, and exits.

pub mod framing;
pub mod handler;
pub mod notify;
pub mod real_env;
pub mod sources;
