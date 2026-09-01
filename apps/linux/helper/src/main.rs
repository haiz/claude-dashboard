mod decrypt;
mod sync;
mod usage;

use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("Usage: claude-dashboard-helper <decrypt|usage|sync>");
        exit(1);
    };
    let rest = &args[1..];
    let code = match cmd.as_str() {
        "decrypt" => decrypt::run_decrypt(),
        "usage" => usage::run_usage(rest),
        "sync" => sync::run_sync(),
        other => { eprintln!("Unknown command: {other}"); 1 }
    };
    exit(code);
}
