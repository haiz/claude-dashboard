mod decrypt;

use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("Usage: claude-dashboard-helper <decrypt|usage|sync>");
        exit(1);
    };
    // Renamed to `_rest` only to keep `cargo clippy -D warnings` clean for
    // Task 9; Task 10 (usage/sync dispatch) restores its use — do not
    // remove the binding.
    let _rest = &args[1..];
    let code = match cmd.as_str() {
        "decrypt" => decrypt::run_decrypt(),
        "usage" => { eprintln!("not yet implemented"); 1 }
        "sync" => { eprintln!("not yet implemented"); 1 }
        other => { eprintln!("Unknown command: {other}"); 1 }
    };
    exit(code);
}
