mod add_key;
mod decrypt;
mod list;
mod sync;
mod usage;

use claude_dashboard_helper::watch;
use std::process::exit;

/// Pinned by `contract/helper-cli.md`, "Dispatch" — the same seven lines the
/// macOS helper prints. `eprint!`, not `eprintln!`: the trailing newline is
/// part of the literal, and a second one would break the byte-exact contract.
///
/// `list` and `remove` are deliberately absent: they are Linux-only commands
/// with no macOS counterpart (the macOS app manages accounts through its own
/// Settings window instead), and this banner is shared contract that must stay
/// byte-identical across the two helpers. They are documented in
/// `contract/helper-cli.md` under "Linux-only commands".
const USAGE_BANNER: &str = "\
Usage: claude-dashboard-helper <command>

Commands:
  decrypt    Decrypt accounts and output JSON to stdout
  sync       Scan installed browsers for Claude sessions and save to accounts
  usage      Fetch usage JSON for an account (args: <orgId> <sessionKey>)
  add-key    Add or repair one account from a session key on stdin
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprint!("{USAGE_BANNER}");
        exit(1);
    };
    let rest = &args[1..];
    let code = match cmd.as_str() {
        "decrypt" => decrypt::run_decrypt(),
        "usage" => usage::run_usage(rest),
        "sync" => sync::run_sync(),
        "add-key" => add_key::run_add_key(),
        "list" => list::run_list(),
        "remove" => list::run_remove(rest),
        "watch" => watch::run_watch(rest),
        other => { eprintln!("Unknown command: {other}"); 1 }
    };
    exit(code);
}
