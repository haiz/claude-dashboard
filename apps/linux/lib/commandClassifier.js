// Ports CommandClassifier.swift's pure half. The shell expansion that feeds
// `expanded` is the caller's job (commandRunner.js), so this file stays free of
// gi:// imports and runs under tests/run.js.
//
// The point of the classifier: a command the user typed is often an opaque
// shell-function name, so the leading token alone cannot say whether the thing
// needs a TTY. The shell resolves it first, and the decision below reads that
// resolution. The verdict is only a default — the user's own toggle wins.

export const KIND = {
    interactive: 'interactive',
    nonInteractive: 'nonInteractive',
};

// Editors, pagers, monitors, multiplexers, REPLs — TUIs that need a terminal.
const INTERACTIVE_TOOLS = [
    'vim', 'vi', 'nvim', 'nano', 'emacs',
    'top', 'htop', 'btop', 'less', 'more',
    'tmux', 'irb', 'lazygit', 'fzf',
];

function firstToken(command) {
    const parts = command.split(' ');
    return parts.length > 0 && parts[0] !== '' ? parts[0] : command;
}

function escapeForRegex(text) {
    return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

// Removes "..." and '...' spans (quotes included), replacing each with a single
// space so the tokens around them are not glued together.
export function stripQuotedRegions(text) {
    let out = '';
    let quote = null;
    for (const ch of text) {
        if (quote === null && (ch === '"' || ch === "'")) {
            // One space for the whole span, emitted at the opening quote — the
            // closing quote adds nothing, matching Swift's single-space
            // replacement of the entire quoted region.
            quote = ch;
            out += ' ';
        } else if (quote !== null && ch === quote) {
            quote = null;
        } else if (quote === null) {
            out += ch;
        }
    }
    return out;
}

// True when the command or its expansion asks for claude's non-interactive
// print mode. Quoted prose is stripped first so `claude "explain the -p flag"`
// is not mistaken for the real flag.
function claudePrintMode(command, expanded) {
    const tokens = stripQuotedRegions(command).split(/\s+/).filter(t => t !== '');
    const hasPrintToken = tokens.some(t =>
        t === '-p' || t === '--print' || t.startsWith('--output-format'));
    const lower = expanded.toLowerCase();
    return hasPrintToken || lower.includes('--print') || lower.includes('--output-format');
}

// ssh is an interactive shell only when no remote command follows the target:
// `ssh host` is interactive, `ssh host uptime` is not. Options and their
// arguments are skipped so `ssh -p 22 host` still reads as interactive.
const SSH_OPTIONS_WITH_ARGUMENT = new Set([
    '-b', '-c', '-D', '-E', '-e', '-F', '-I', '-i', '-J', '-L', '-l', '-m',
    '-O', '-o', '-p', '-Q', '-R', '-S', '-W', '-w',
]);

export function sshInteractive(command) {
    const tokens = stripQuotedRegions(command).split(/\s+/).filter(t => t !== '');
    const sshIndex = tokens.findIndex(t => t === 'ssh' || t.endsWith('/ssh'));
    if (sshIndex < 0)
        return true;

    let i = sshIndex + 1;
    let sawTarget = false;
    while (i < tokens.length) {
        const token = tokens[i];
        if (token.startsWith('-')) {
            i += SSH_OPTIONS_WITH_ARGUMENT.has(token) ? 2 : 1;
            continue;
        }
        if (!sawTarget) {
            sawTarget = true;
            i += 1;
            continue;
        }
        // A token past the target is a remote command.
        return false;
    }
    return true;
}

export function classify(command, expanded) {
    // Name matching must never see argument prose (e.g. `echo "no more files"`),
    // so the haystack is the resolved expansion plus only the leading token.
    const haystack = `${firstToken(command)}\n${expanded}`.toLowerCase();
    const word = w => new RegExp(`\\b${escapeForRegex(w)}\\b`).test(haystack);

    if (word('claude'))
        return claudePrintMode(command, expanded) ? KIND.nonInteractive : KIND.interactive;

    for (const tool of INTERACTIVE_TOOLS) {
        if (word(tool))
            return KIND.interactive;
    }

    if (word('ssh'))
        return sshInteractive(command) ? KIND.interactive : KIND.nonInteractive;

    return KIND.nonInteractive;
}
