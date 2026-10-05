//! Ports `CommandClassifier.swift`'s pure half (and the Linux port's refined
//! ssh walk, `apps/linux/lib/commandClassifier.js`). The caller expands the
//! leading token through the user's shell first (`expanded`); this only reads
//! the result. The verdict is a default — the user's toggle wins.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Interactive,
    NonInteractive,
}

/// Editors, pagers, monitors, multiplexers, REPLs — TUIs that need a terminal.
const INTERACTIVE_TOOLS: &[&str] = &[
    "vim", "vi", "nvim", "nano", "emacs", "top", "htop", "btop", "less", "more", "tmux", "irb",
    "lazygit", "fzf",
];

/// ssh short options that consume the following token as their value.
const SSH_OPTIONS_WITH_ARGUMENT: &[&str] = &[
    "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p", "-Q",
    "-R", "-S", "-W", "-w",
];

pub fn leading_token(command: &str) -> &str {
    command.split(' ').next().unwrap_or(command)
}

/// The leading token, only if it looks like a bare command name or path.
/// Anything with shell metacharacters is refused so it is never interpolated
/// into (and run by) the resolver's script.
pub fn resolvable_token(command: &str) -> Option<&str> {
    let t = leading_token(command);
    let ok = !t.is_empty()
        && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '\\' | ':' | '-'));
    ok.then_some(t)
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `\bword\b` over ASCII word chars (`word` is ASCII).
fn has_word(haystack: &str, word: &str) -> bool {
    let h = haystack.as_bytes();
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(word) {
        let i = from + pos;
        let end = i + word.len();
        let before_ok = i == 0 || !is_word_byte(h[i - 1]);
        let after_ok = end >= h.len() || !is_word_byte(h[end]);
        if before_ok && after_ok {
            return true;
        }
        from = i + 1;
        while from < haystack.len() && !haystack.is_char_boundary(from) {
            from += 1;
        }
    }
    false
}

/// Replaces each `"..."` / `'...'` span (quotes included) with `replacement`.
fn replace_quoted_regions(s: &str, replacement: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut quote: Option<char> = None;
    for ch in s.chars() {
        match quote {
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                out.push_str(replacement);
            }
            None => out.push(ch),
            Some(q) if ch == q => quote = None,
            Some(_) => {}
        }
    }
    out
}

/// Removes `"..."` and `'...'` spans (quotes included), one space per span.
pub fn strip_quoted_regions(s: &str) -> String {
    replace_quoted_regions(s, " ")
}

fn claude_print_mode(command: &str, expanded: &str) -> bool {
    let stripped = strip_quoted_regions(command);
    let has_token = stripped
        .split_whitespace()
        .any(|t| t == "-p" || t == "--print" || t.starts_with("--output-format"));
    let lower = expanded.to_lowercase();
    has_token || lower.contains("--print") || lower.contains("--output-format")
}

/// `ssh host` is interactive; `ssh host cmd…` runs a remote command.
/// A quoted span stays one positional token (`" _ "`) so `ssh host "df -h"`
/// counts as a remote command rather than vanishing.
pub fn ssh_interactive(command: &str) -> bool {
    let stripped = replace_quoted_regions(command, " _ ");
    let tokens: Vec<&str> = stripped.split_whitespace().collect();
    let Some(idx) = tokens.iter().position(|t| {
        *t == "ssh" || t.ends_with("/ssh") || t.ends_with("\\ssh") || t.ends_with("ssh.exe")
    }) else {
        return true;
    };
    let mut i = idx + 1;
    let mut saw_target = false;
    while i < tokens.len() {
        let t = tokens[i];
        if t.starts_with('-') {
            i += if SSH_OPTIONS_WITH_ARGUMENT.contains(&t) { 2 } else { 1 };
            continue;
        }
        if !saw_target {
            saw_target = true;
            i += 1;
            continue;
        }
        return false;
    }
    true
}

pub fn classify(command: &str, expanded: &str) -> CommandKind {
    // Name matching never sees argument prose: only the leading token plus the
    // shell's resolution of it.
    let haystack = format!("{}\n{}", leading_token(command), expanded).to_lowercase();
    if has_word(&haystack, "claude") {
        return if claude_print_mode(command, expanded) {
            CommandKind::NonInteractive
        } else {
            CommandKind::Interactive
        };
    }
    if INTERACTIVE_TOOLS.iter().any(|t| has_word(&haystack, t)) {
        return CommandKind::Interactive;
    }
    if has_word(&haystack, "ssh") {
        return if ssh_interactive(command) {
            CommandKind::Interactive
        } else {
            CommandKind::NonInteractive
        };
    }
    CommandKind::NonInteractive
}

#[cfg(test)]
mod tests {
    use super::*;
    use CommandKind::*;

    #[test]
    fn plain_commands_are_non_interactive() {
        assert_eq!(classify("ls -la", ""), NonInteractive);
        assert_eq!(classify("git status", "C:\\Program Files\\Git\\cmd\\git.exe"), NonInteractive);
    }

    #[test]
    fn tuis_are_interactive() {
        for c in ["vim notes.md", "nvim", "htop", "less log.txt", "lazygit", "fzf"] {
            assert_eq!(classify(c, ""), Interactive, "{c}");
        }
    }

    #[test]
    fn word_boundaries_hold() {
        // `vi` must not match `video`; `more` must not match `moreutils`.
        assert_eq!(classify("video-convert in.mp4", ""), NonInteractive);
        assert_eq!(classify("moreutils-thing", ""), NonInteractive);
        // Windows paths and extensions are boundaries.
        assert_eq!(classify("C:\\tools\\htop.exe", ""), Interactive);
        assert_eq!(classify("claude.exe", ""), Interactive);
    }

    #[test]
    fn argument_prose_is_ignored() {
        assert_eq!(classify("echo \"no more files\"", ""), NonInteractive);
        assert_eq!(classify("echo vim", ""), NonInteractive);
    }

    #[test]
    fn claude_is_interactive_unless_print_mode() {
        assert_eq!(classify("claude", ""), Interactive);
        assert_eq!(classify("claude -p \"hi\"", ""), NonInteractive);
        assert_eq!(classify("claude --print hi", ""), NonInteractive);
        assert_eq!(classify("claude --output-format=json -x", ""), NonInteractive);
        assert_eq!(classify("claude \"explain the -p flag\"", ""), Interactive);
    }

    #[test]
    fn expansion_reveals_the_real_binary() {
        let body = "ccbf\n{\n    claude --dangerously-skip-permissions\n}";
        assert_eq!(classify("ccbf", body), Interactive);
        let printing = "function ccp { claude --print $args }";
        assert_eq!(classify("ccp hello", printing), NonInteractive);
    }

    #[test]
    fn ssh_without_remote_command_is_interactive() {
        assert_eq!(classify("ssh host", ""), Interactive);
        assert_eq!(classify("ssh -p 22 host", ""), Interactive);
        assert_eq!(classify("ssh -i key.pem user@host", ""), Interactive);
        assert_eq!(classify("ssh host uptime", ""), NonInteractive);
        assert_eq!(classify("ssh -p 22 host \"df -h\"", ""), NonInteractive);
    }

    #[test]
    fn strip_quoted_regions_replaces_each_span_with_one_space() {
        assert_eq!(strip_quoted_regions("a \"b c\" d"), "a   d");
        assert_eq!(strip_quoted_regions("x 'y' z"), "x   z");
        assert_eq!(strip_quoted_regions("open \"unterminated"), "open  ");
    }

    #[test]
    fn leading_token_is_first_space_token() {
        assert_eq!(leading_token("git status"), "git");
        assert_eq!(leading_token("ccbf"), "ccbf");
        assert_eq!(leading_token(""), "");
    }

    #[test]
    fn resolvable_token_accepts_names_and_paths() {
        assert_eq!(resolvable_token("ccbf --x"), Some("ccbf"));
        assert_eq!(resolvable_token("C:\\tools\\a-b_c.exe arg"), Some("C:\\tools\\a-b_c.exe"));
        assert_eq!(resolvable_token("./run.sh"), Some("./run.sh"));
    }

    #[test]
    fn resolvable_token_refuses_metacharacters() {
        for c in ["$(rm -rf ~)", "`id`", "a;b", "a|b", "a&b", "'x'", "\"x\"", "%PATH%", "", "a>b", "(x)"] {
            assert_eq!(resolvable_token(c), None, "{c:?}");
        }
    }
}
