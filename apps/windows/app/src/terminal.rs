//! Interactive terminal launch (Windows Terminal, else conhost).

use crate::shell::{Invocation, ShellKind, ShellSpec};
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`; existence is not checked.
pub fn wt_path(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let local = env("LOCALAPPDATA")?;
    Some(Path::new(&local).join("Microsoft").join("WindowsApps").join("wt.exe"))
}

/// wt treats a bare `;` as its own subcommand separator.
pub fn wt_escape(arg: &str) -> String {
    arg.replace(';', "\\;")
}

pub fn terminal_invocation(
    spec: &ShellSpec,
    command: &str,
    wt: Option<&Path>,
    system_root: &Path,
) -> Invocation {
    let exe = spec.exe.to_string_lossy().into_owned();
    // cmd.exe parses its own command line, so its `/k "<command>"` tail must
    // bypass Rust's argv quoting and travel as raw_args.
    if spec.kind == ShellKind::Cmd {
        let raw = format!("/k \"{command}\"");
        return match wt {
            Some(p) => {
                let mut args: Vec<String> = ["new-tab", "--title", "Claude Dashboard", "--"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                args.push(wt_escape(&exe));
                Invocation { program: p.to_path_buf(), args, raw_args: Some(wt_escape(&raw)) }
            }
            None => Invocation {
                program: system_root.join("System32").join("conhost.exe"),
                args: vec![exe],
                raw_args: Some(raw),
            },
        };
    }
    let tail: Vec<String> = if spec.kind == ShellKind::GitBash {
        vec![exe, "-l".into(), "-i".into(), "-c".into(), format!("{command}\nexec bash -l -i")]
    } else {
        // PowerShell (pwsh / Windows PowerShell); cmd returned above.
        vec![exe, "-NoLogo".into(), "-NoExit".into(), "-Command".into(), command.into()]
    };
    match wt {
        Some(p) => {
            let mut args: Vec<String> = ["new-tab", "--title", "Claude Dashboard", "--"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            args.extend(tail.iter().map(|a| wt_escape(a)));
            Invocation { program: p.to_path_buf(), args, raw_args: None }
        }
        None => Invocation {
            program: system_root.join("System32").join("conhost.exe"),
            args: tail,
            raw_args: None,
        },
    }
}

/// Spawn detached: no job object, no wait.
pub fn launch(inv: &Invocation) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(&inv.program);
    c.args(&inv.args);
    if let Some(raw) = &inv.raw_args {
        c.raw_arg(raw);
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        c.current_dir(home);
    }
    c.spawn().map(|_| ()).map_err(|e| format!("terminal launch failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{ShellKind, ShellSpec};

    fn spec(kind: ShellKind, exe: &str) -> ShellSpec { ShellSpec { kind, exe: PathBuf::from(exe) } }
    const ROOT: &str = "C:\\Windows";

    #[test]
    fn wt_escapes_semicolons() {
        assert_eq!(wt_escape("a; b;c"), "a\\; b\\;c");
        assert_eq!(wt_escape("plain"), "plain");
    }

    #[test]
    fn wt_new_tab_with_noexit_pwsh() {
        let wt = PathBuf::from("C:\\wt.exe");
        let i = terminal_invocation(&spec(ShellKind::Pwsh, "C:\\pwsh.exe"), "claude; echo done", Some(&wt), Path::new(ROOT));
        assert_eq!(i.program, wt);
        assert_eq!(i.args, ["new-tab", "--title", "Claude Dashboard", "--", "C:\\pwsh.exe",
                            "-NoLogo", "-NoExit", "-Command", "claude\\; echo done"]);
        assert_eq!(i.raw_args, None);
    }

    #[test]
    fn conhost_fallback_without_wt() {
        let i = terminal_invocation(&spec(ShellKind::Cmd, "C:\\cmd.exe"), "dir", None, Path::new(ROOT));
        assert_eq!(i.program, PathBuf::from("C:\\Windows\\System32\\conhost.exe"));
        assert_eq!(i.args, ["C:\\cmd.exe"]);
        assert_eq!(i.raw_args.as_deref(), Some("/k \"dir\""));
    }

    #[test]
    fn cmd_tail_is_raw_with_quotes_and_ampersand_intact() {
        let c = "git commit -m \"x\" & echo y";
        let i = terminal_invocation(&spec(ShellKind::Cmd, "C:\\cmd.exe"), c, None, Path::new(ROOT));
        assert_eq!(i.raw_args.as_deref(), Some("/k \"git commit -m \"x\" & echo y\""));
        let wt = PathBuf::from("C:\\wt.exe");
        let i = terminal_invocation(&spec(ShellKind::Cmd, "C:\\cmd.exe"), "a; b", Some(&wt), Path::new(ROOT));
        assert_eq!(i.args, ["new-tab", "--title", "Claude Dashboard", "--", "C:\\cmd.exe"]);
        assert_eq!(i.raw_args.as_deref(), Some("/k \"a\\; b\""));
    }

    #[test]
    fn git_bash_keeps_an_interactive_shell_after_the_command() {
        let i = terminal_invocation(&spec(ShellKind::GitBash, "C:\\bash.exe"), "htop", None, Path::new(ROOT));
        assert_eq!(i.args, ["C:\\bash.exe", "-l", "-i", "-c", "htop\nexec bash -l -i"]);
    }

    #[test]
    fn wt_path_under_windowsapps() {
        let env = |k: &str| (k == "LOCALAPPDATA").then(|| "C:\\L".to_string());
        assert_eq!(wt_path(&env), Some(PathBuf::from("C:\\L\\Microsoft\\WindowsApps\\wt.exe")));
    }
}
