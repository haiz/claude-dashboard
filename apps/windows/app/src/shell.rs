//! Shell discovery and argv building for hidden runs, resolver scripts and
//! (via `terminal.rs`) interactive launches.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Pwsh,
    WindowsPowerShell,
    Cmd,
    GitBash,
}

impl ShellKind {
    #[allow(dead_code)] // used by the shell picker (Task 11)
    pub const ALL: [ShellKind; 4] = [
        ShellKind::Pwsh,
        ShellKind::WindowsPowerShell,
        ShellKind::Cmd,
        ShellKind::GitBash,
    ];

    #[allow(dead_code)] // used by the shell picker (Task 11)
    pub fn setting_key(self) -> &'static str {
        match self {
            ShellKind::Pwsh => "pwsh",
            ShellKind::WindowsPowerShell => "powershell",
            ShellKind::Cmd => "cmd",
            ShellKind::GitBash => "bash",
        }
    }

    pub fn from_setting(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.setting_key() == s)
    }

    #[allow(dead_code)] // used by the shell picker (Task 11)
    pub fn label(self) -> &'static str {
        match self {
            ShellKind::Pwsh => "PowerShell 7",
            ShellKind::WindowsPowerShell => "Windows PowerShell",
            ShellKind::Cmd => "Command Prompt",
            ShellKind::GitBash => "Git Bash",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellSpec {
    pub kind: ShellKind,
    pub exe: PathBuf,
}

/// `raw_args` is appended verbatim after `args` (`CommandExt::raw_arg`);
/// `cmd.exe` needs it because it parses its own command line.
#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub raw_args: Option<String>,
}

pub struct Probe<'a> {
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub exists: &'a dyn Fn(&Path) -> bool,
}

pub fn locate(kind: ShellKind, p: &Probe) -> Option<PathBuf> {
    let ok = |c: PathBuf| (p.exists)(&c).then_some(c);
    match kind {
        ShellKind::Pwsh => {
            if let Some(path) = (p.env)("PATH") {
                for dir in path.split(';').filter(|d| !d.is_empty()) {
                    if let Some(found) = ok(Path::new(dir).join("pwsh.exe")) {
                        return Some(found);
                    }
                }
            }
            let pf = (p.env)("ProgramFiles")?;
            ok(Path::new(&pf).join("PowerShell").join("7").join("pwsh.exe"))
        }
        ShellKind::WindowsPowerShell => {
            let root = (p.env)("SystemRoot")?;
            ok(Path::new(&root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"))
        }
        ShellKind::Cmd => {
            if let Some(found) = (p.env)("ComSpec").and_then(|c| ok(PathBuf::from(c))) {
                return Some(found);
            }
            let root = (p.env)("SystemRoot")?;
            ok(Path::new(&root).join("System32").join("cmd.exe"))
        }
        ShellKind::GitBash => {
            let rel = |base: String, sub: &[&str]| {
                let mut path = PathBuf::from(base);
                path.extend(sub);
                path
            };
            if let Some(found) = (p.env)("ProgramFiles")
                .and_then(|pf| ok(rel(pf, &["Git", "bin", "bash.exe"])))
            {
                return Some(found);
            }
            let la = (p.env)("LOCALAPPDATA")?;
            ok(rel(la, &["Programs", "Git", "bin", "bash.exe"]))
        }
    }
}

pub fn available(p: &Probe) -> Vec<ShellSpec> {
    ShellKind::ALL
        .into_iter()
        .filter_map(|kind| locate(kind, p).map(|exe| ShellSpec { kind, exe }))
        .collect()
}

pub fn resolve(setting: Option<&str>, p: &Probe) -> Option<ShellSpec> {
    let spec_for = |kind: ShellKind| locate(kind, p).map(|exe| ShellSpec { kind, exe });
    setting
        .and_then(ShellKind::from_setting)
        .and_then(spec_for)
        .or_else(|| {
            [ShellKind::Pwsh, ShellKind::WindowsPowerShell, ShellKind::Cmd]
                .into_iter()
                .find_map(spec_for)
        })
}

fn real_env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn real_exists(p: &Path) -> bool {
    p.is_file()
}

pub fn detect(setting: Option<&str>) -> Option<ShellSpec> {
    resolve(setting, &Probe { env: &real_env, exists: &real_exists })
}

#[allow(dead_code)] // used by the shell picker in settings_general.rs (Task 11)
pub fn detect_available() -> Vec<ShellSpec> {
    available(&Probe { env: &real_env, exists: &real_exists })
}

fn inv(spec: &ShellSpec, args: Vec<String>, raw_args: Option<String>) -> Invocation {
    Invocation { program: spec.exe.clone(), args, raw_args }
}

fn strs(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// A hidden run that loads the shell profile.
pub fn run_invocation(spec: &ShellSpec, command: &str) -> Invocation {
    match spec.kind {
        ShellKind::Pwsh | ShellKind::WindowsPowerShell => {
            let mut args = strs(&["-NoLogo", "-NonInteractive", "-Command"]);
            args.push(format!(
                "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; {command}"
            ));
            inv(spec, args, None)
        }
        ShellKind::Cmd => inv(spec, strs(&["/s", "/c"]), Some(format!("\"{command}\""))),
        ShellKind::GitBash => inv(
            spec,
            vec!["-c".into(), format!("source ~/.bashrc 2>/dev/null\n{command}")],
            None,
        ),
    }
}

/// One level of alias/function expansion. The token must already be validated
/// by `command_classifier::resolvable_token`.
pub fn resolve_invocation(spec: &ShellSpec, token: &str) -> Invocation {
    debug_assert!(
        claude_dashboard_core::command_classifier::resolvable_token(token) == Some(token)
    );
    match spec.kind {
        ShellKind::Pwsh | ShellKind::WindowsPowerShell => {
            let mut args = strs(&["-NoLogo", "-NonInteractive", "-Command"]);
            args.push(format!(
                "$c = Get-Command -Name '{token}' -ErrorAction SilentlyContinue | Select-Object -First 1; if ($c) {{ $c.Definition; $c.Source }}"
            ));
            inv(spec, args, None)
        }
        ShellKind::Cmd => inv(spec, strs(&["/s", "/c"]), Some(format!("\"where {token}\""))),
        ShellKind::GitBash => inv(
            spec,
            vec!["-ic".into(), format!("type {token} 2>/dev/null")],
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    struct Fake { env: HashMap<&'static str, String>, files: HashSet<PathBuf> }
    impl Fake {
        fn new(env: &[(&'static str, &str)], files: &[&str]) -> Self {
            Fake {
                env: env.iter().map(|(k, v)| (*k, v.to_string())).collect(),
                files: files.iter().map(PathBuf::from).collect(),
            }
        }
        fn with<R>(&self, f: impl FnOnce(&Probe) -> R) -> R {
            let env = |k: &str| self.env.get(k).cloned();
            let exists = |p: &Path| self.files.contains(p);
            f(&Probe { env: &env, exists: &exists })
        }
    }

    const BASE: &[(&str, &str)] = &[
        ("SystemRoot", "C:\\Windows"),
        ("ProgramFiles", "C:\\Program Files"),
        ("LOCALAPPDATA", "C:\\Users\\u\\AppData\\Local"),
        ("ComSpec", "C:\\Windows\\system32\\cmd.exe"),
        ("PATH", "C:\\Windows\\system32;;C:\\tools"),
    ];
    const WINPS: &str = "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
    const CMD: &str = "C:\\Windows\\system32\\cmd.exe";

    #[test]
    fn pwsh_found_on_path_first() {
        let f = Fake::new(BASE, &["C:\\tools\\pwsh.exe", "C:\\Program Files\\PowerShell\\7\\pwsh.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::Pwsh, p)), Some(PathBuf::from("C:\\tools\\pwsh.exe")));
    }

    #[test]
    fn pwsh_falls_back_to_program_files() {
        let f = Fake::new(BASE, &["C:\\Program Files\\PowerShell\\7\\pwsh.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::Pwsh, p)),
                   Some(PathBuf::from("C:\\Program Files\\PowerShell\\7\\pwsh.exe")));
    }

    #[test]
    fn git_bash_never_picks_wsl_bash() {
        let f = Fake::new(BASE, &["C:\\Windows\\System32\\bash.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::GitBash, p)), None);
        let f = Fake::new(BASE, &["C:\\Program Files\\Git\\bin\\bash.exe"]);
        assert_eq!(f.with(|p| locate(ShellKind::GitBash, p)),
                   Some(PathBuf::from("C:\\Program Files\\Git\\bin\\bash.exe")));
    }

    #[test]
    fn default_prefers_pwsh_then_windows_powershell() {
        let f = Fake::new(BASE, &[WINPS, CMD]);
        assert_eq!(f.with(|p| resolve(None, p)).map(|s| s.kind), Some(ShellKind::WindowsPowerShell));
        let f = Fake::new(BASE, &["C:\\tools\\pwsh.exe", WINPS, CMD]);
        assert_eq!(f.with(|p| resolve(None, p)).map(|s| s.kind), Some(ShellKind::Pwsh));
    }

    #[test]
    fn chosen_shell_used_when_available_else_default() {
        let f = Fake::new(BASE, &[WINPS, CMD]);
        assert_eq!(f.with(|p| resolve(Some("cmd"), p)).map(|s| s.kind), Some(ShellKind::Cmd));
        assert_eq!(f.with(|p| resolve(Some("bash"), p)).map(|s| s.kind), Some(ShellKind::WindowsPowerShell));
        assert_eq!(f.with(|p| resolve(Some("garbage"), p)).map(|s| s.kind), Some(ShellKind::WindowsPowerShell));
    }

    #[test]
    fn nothing_found_is_none() {
        let f = Fake::new(&[], &[]);
        assert_eq!(f.with(|p| resolve(None, p)), None);
        assert!(f.with(available).is_empty());
    }

    #[test]
    fn setting_keys_roundtrip() {
        for k in ShellKind::ALL {
            assert_eq!(ShellKind::from_setting(k.setting_key()), Some(k));
        }
        assert_eq!(ShellKind::from_setting("zsh"), None);
    }

    fn spec(kind: ShellKind, exe: &str) -> ShellSpec { ShellSpec { kind, exe: PathBuf::from(exe) } }

    #[test]
    fn run_invocations_load_profile_and_pass_command() {
        let i = run_invocation(&spec(ShellKind::Pwsh, "pwsh.exe"), "ccbf --x");
        assert_eq!(i.args[..3], ["-NoLogo", "-NonInteractive", "-Command"]);
        assert!(i.args[3].ends_with("; ccbf --x"));
        assert!(!i.args.iter().any(|a| a == "-NoProfile"));
        let i = run_invocation(&spec(ShellKind::Cmd, CMD), "echo a & echo b");
        assert_eq!(i.args, ["/s", "/c"]);
        assert_eq!(i.raw_args.as_deref(), Some("\"echo a & echo b\""));
        let i = run_invocation(&spec(ShellKind::GitBash, "bash.exe"), "ls");
        assert_eq!(i.args, ["-c", "source ~/.bashrc 2>/dev/null\nls"]);
    }

    #[test]
    fn resolve_invocations_embed_only_the_token() {
        let i = resolve_invocation(&spec(ShellKind::Pwsh, "pwsh.exe"), "ccbf");
        assert!(i.args[3].contains("Get-Command -Name 'ccbf'"));
        let i = resolve_invocation(&spec(ShellKind::Cmd, CMD), "git");
        assert_eq!(i.raw_args.as_deref(), Some("\"where git\""));
        let i = resolve_invocation(&spec(ShellKind::GitBash, "bash.exe"), "ll");
        assert_eq!(i.args, ["-ic", "type ll 2>/dev/null"]);
    }
}
