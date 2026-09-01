use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else {
        eprintln!("Usage: claude-dashboard-helper <decrypt|usage|sync>");
        exit(1);
    };
    let rest = &args[1..];
    let code = match cmd.as_str() {
        "decrypt" => { eprintln!("not yet implemented"); 1 }
        "usage" => { eprintln!("not yet implemented"); 1 }
        "sync" => { eprintln!("not yet implemented"); 1 }
        other => { eprintln!("Unknown command: {other}"); 1 }
    };
    exit(code);
}
