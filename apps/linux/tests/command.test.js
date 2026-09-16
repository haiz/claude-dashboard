import {test, assertEqual} from './harness.js';
import {classify, sshInteractive, stripQuotedRegions, KIND} from '../lib/commandClassifier.js';
import {
    CommandLog, TRIGGER, STATUS, TRIGGER_LABEL, STATUS_LABEL,
    boundedTail, serialize, deserialize, MAX_OUTPUT_BYTES,
} from '../lib/commandLog.js';

// --- Classifier ---

test('a bare claude needs a terminal', () => {
    assertEqual(classify('claude', '/usr/local/bin/claude'), KIND.interactive);
});

test('claude in print mode does not', () => {
    assertEqual(classify('claude -p "hi"', '/usr/local/bin/claude'), KIND.nonInteractive);
    assertEqual(classify('claude --print hi', '/usr/local/bin/claude'), KIND.nonInteractive);
    assertEqual(classify('claude --output-format=json hi', '/usr/local/bin/claude'), KIND.nonInteractive);
});

test('quoted prose is never mistaken for a print flag', () => {
    // The regression this guards: `-p` inside the prompt text, not as a flag.
    assertEqual(classify('claude "explain the -p flag"', '/usr/local/bin/claude'), KIND.interactive);
});

test('an opaque shell function is classified by what it expands to', () => {
    // The user typed `ccbf`; only the expansion reveals claude --print.
    assertEqual(classify('ccbf', 'ccbf () { claude --print "$@" }'), KIND.nonInteractive);
    assertEqual(classify('ccx', 'ccx () { claude "$@" }'), KIND.interactive);
});

test('TUI tools are interactive', () => {
    for (const tool of ['vim', 'nvim', 'htop', 'less', 'tmux', 'lazygit', 'fzf'])
        assertEqual(classify(tool, `/usr/bin/${tool}`), KIND.interactive, tool);
});

test('argument prose cannot name a TUI into the verdict', () => {
    // "less" appears only as an argument, never as the command.
    assertEqual(classify('echo "use less paper"', '/usr/bin/echo'), KIND.nonInteractive);
});

test('ssh is interactive only without a remote command', () => {
    assertEqual(classify('ssh host', '/usr/bin/ssh'), KIND.interactive);
    assertEqual(classify('ssh host uptime', '/usr/bin/ssh'), KIND.nonInteractive);
});

test('ssh options and their arguments do not count as the remote command', () => {
    assertEqual(sshInteractive('ssh -p 22 host'), true);
    assertEqual(sshInteractive('ssh -i ~/.ssh/id_ed25519 host'), true);
    assertEqual(sshInteractive('ssh -v host'), true);
    assertEqual(sshInteractive('ssh -p 22 host uptime'), false);
});

test('anything else runs without a terminal', () => {
    assertEqual(classify('ls -la', '/usr/bin/ls'), KIND.nonInteractive);
    assertEqual(classify('git status', '/usr/bin/git'), KIND.nonInteractive);
});

test('quoted regions collapse to a space rather than vanishing', () => {
    assertEqual(stripQuotedRegions('a "b c" d'), 'a   d');
    assertEqual(stripQuotedRegions("a 'b' d"), 'a   d');
});

// --- Command log ---

const T0 = 1_700_000_000_000;

function entry(log, overrides = {}) {
    return log.record({
        command: 'ls',
        trigger: TRIGGER.manual,
        startedAtMs: T0,
        finishedAtMs: T0 + 1000,
        status: STATUS.exited,
        exitCode: 0,
        ...overrides,
    });
}

test('the trigger and status vocabularies match CommandLogModels', () => {
    assertEqual(TRIGGER.manual, 0);
    assertEqual(TRIGGER.autoReset, 1);
    assertEqual(TRIGGER.autoEmpty, 2);
    assertEqual(STATUS.exited, 0);
    assertEqual(STATUS.launchedInTerminal, 3);
    assertEqual(STATUS.launchFailed, 4);
    assertEqual(TRIGGER_LABEL[1], 'Auto (reset)');
    assertEqual(STATUS_LABEL[3], 'In Terminal');
});

test('entries list newest first', () => {
    const log = new CommandLog();
    entry(log, {command: 'first'});
    entry(log, {command: 'second'});
    assertEqual(log.list().map(e => e.command).join(','), 'second,first');
});

test('the log is capped at its maximum, keeping the newest', () => {
    const log = new CommandLog([], 3);
    for (let i = 0; i < 5; i++)
        entry(log, {command: `cmd${i}`});
    assertEqual(log.entries.length, 3);
    assertEqual(log.list().map(e => e.command).join(','), 'cmd4,cmd3,cmd2');
});

test('empty output is stored as null rather than an empty string', () => {
    const log = new CommandLog();
    assertEqual(entry(log, {output: ''}).output, null);
    assertEqual(entry(log, {output: null}).output, null);
    assertEqual(entry(log, {output: 'hi'}).output, 'hi');
});

test('only the tail of a long output is kept', () => {
    const long = 'x'.repeat(MAX_OUTPUT_BYTES + 500) + 'END';
    const kept = boundedTail(long);
    assertEqual(new TextEncoder().encode(kept).length <= MAX_OUTPUT_BYTES, true);
    assertEqual(kept.endsWith('END'), true);
});

test('the tail cap is measured in bytes and never leaves a split code point', () => {
    const kept = boundedTail('é'.repeat(4000), 16);
    assertEqual(new TextEncoder().encode(kept).length <= 16, true);
    assertEqual(kept.includes('�'), false);
});

test('clear empties the log and reports how many rows went', () => {
    const log = new CommandLog();
    entry(log);
    entry(log);
    assertEqual(log.clear(), 2);
    assertEqual(log.entries.length, 0);
});

test('a log survives a round trip and keeps minting fresh ids', () => {
    const log = new CommandLog();
    entry(log, {command: 'a'});
    const back = deserialize(serialize(log));
    assertEqual(back.entries.length, 1);
    assertEqual(entry(back, {command: 'b'}).id > log.entries[0].id, true);
});

test('a malformed or unknown-version document reads as an empty log', () => {
    assertEqual(deserialize('nope').entries.length, 0);
    assertEqual(deserialize('{"version":9,"entries":[]}').entries.length, 0);
    assertEqual(deserialize('{"version":1,"entries":[{"id":"x"}]}').entries.length, 0);
});
