// Ports CommandRunner.swift, ProcessTree.swift and TerminalLauncher.swift.
//
// macOS runs `/bin/zsh -c "source ~/.zshrc; <command>"` so the user's shell
// functions resolve, kills the whole process tree on timeout so grandchildren
// like `caffeinate` do not leak, keeps a bounded output tail, and records one
// row per run. All of that is reproduced here; only the platform mechanics
// differ — /proc instead of libproc for the tree walk, and a terminal emulator
// launched directly instead of AppleScript.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';

import {classify, KIND} from './lib/commandClassifier.js';
import {STATUS, boundedTail} from './lib/commandLog.js';

// CommandRunner.swift's `timeout: TimeInterval = 60`, and its two-second grace
// between SIGTERM and SIGKILL.
const DEFAULT_TIMEOUT_SECONDS = 60;
const KILL_GRACE_SECONDS = 2;

// --- Process tree ------------------------------------------------------
//
// Gio.Subprocess.force_exit() signals only the direct child, exactly the gap
// ProcessTree.swift exists to close. Linux exposes the children of a pid in
// /proc/<pid>/task/*/children, which is cheaper and more direct than scanning
// every /proc/*/stat for a matching ppid.

function childPids(pid) {
    const out = [];
    const taskDir = `/proc/${pid}/task`;
    let names;
    try {
        const dir = Gio.File.new_for_path(taskDir);
        const enumerator = dir.enumerate_children('standard::name', Gio.FileQueryInfoFlags.NONE, null);
        names = [];
        let info;
        while ((info = enumerator.next_file(null)) !== null)
            names.push(info.get_name());
        enumerator.close(null);
    } catch {
        return out;
    }

    for (const tid of names) {
        try {
            const [ok, bytes] = GLib.file_get_contents(`${taskDir}/${tid}/children`);
            if (!ok)
                continue;
            for (const token of new TextDecoder().decode(bytes).split(/\s+/)) {
                const child = parseInt(token, 10);
                if (Number.isInteger(child) && child > 0)
                    out.push(child);
            }
        } catch {
            // A task that exited between listing and reading is not an error.
        }
    }
    return out;
}

export function descendants(pid) {
    const out = [];
    const stack = childPids(pid);
    // A cycle is impossible in a process tree, but a pid reused mid-walk could
    // still produce one; `seen` makes the walk total either way.
    const seen = new Set();
    while (stack.length > 0) {
        const next = stack.pop();
        if (seen.has(next))
            continue;
        seen.add(next);
        out.push(next);
        stack.push(...childPids(next));
    }
    return out;
}

export function killTree(pid, signal) {
    // Descendants first, then the root: killing the root first would reparent
    // the rest to init and lose them.
    for (const child of descendants(pid)) {
        try {
            GLib.spawn_command_line_sync(`kill -${signal} ${child}`);
        } catch {
            // kill on an already-dead pid is a harmless no-op.
        }
    }
    try {
        GLib.spawn_command_line_sync(`kill -${signal} ${pid}`);
    } catch {
        // As above.
    }
}

// --- Shell -------------------------------------------------------------

function userShell() {
    const shell = GLib.getenv('SHELL');
    return shell && shell !== '' ? shell : '/bin/sh';
}

// macOS hardcodes `source ~/.zshrc 2>/dev/null`. On Linux the login shell
// varies, so the matching rc file is chosen from $SHELL; sourcing the wrong one
// is harmless because the redirect swallows the error, but sourcing the right
// one is what makes the user's functions and PATH resolve.
function rcFileFor(shell) {
    if (shell.endsWith('zsh'))
        return '~/.zshrc';
    if (shell.endsWith('bash'))
        return '~/.bashrc';
    if (shell.endsWith('fish'))
        return '~/.config/fish/config.fish';
    return '';
}

export function shellArgv(command, shell = userShell()) {
    const rc = rcFileFor(shell);
    const prelude = rc === '' ? '' : `source ${rc} 2>/dev/null\n`;
    return [shell, '-c', `${prelude}${command}`];
}

// --- Terminal launcher -------------------------------------------------
//
// TerminalLauncher.swift prefers iTerm, else Terminal.app, via AppleScript.
// Linux has no single answer, so the first terminal on PATH wins; each entry
// carries the flag that makes it run a command, since they disagree.
const TERMINALS = [
    ['kgx', '-e'],                  // GNOME Console, the GNOME 46 default
    ['gnome-terminal', '--'],
    ['konsole', '-e'],
    ['xfce4-terminal', '-x'],
    ['alacritty', '-e'],
    ['kitty', '-e'],
    ['wezterm', '-e'],
    ['foot', '-e'],
    ['xterm', '-e'],
    ['x-terminal-emulator', '-e'],  // the Debian alternatives symlink, last
];

export function findTerminal(lookup = GLib.find_program_in_path) {
    for (const [name, flag] of TERMINALS) {
        const path = lookup(name);
        if (path)
            return {path, flag, name};
    }
    return null;
}

// The command is handed to the user's shell as a single argument so quoting,
// pipes and functions behave the same as in the non-interactive path. `exec`
// keeps the window tied to the command rather than leaving a shell behind.
export function terminalArgv(terminal, command, shell = userShell()) {
    return [terminal.path, terminal.flag, shell, '-ic', command];
}

// --- Runner ------------------------------------------------------------

export class CommandRunner {
    constructor({store, timeoutSeconds = DEFAULT_TIMEOUT_SECONDS} = {}) {
        this._store = store;
        this._timeoutSeconds = timeoutSeconds;
        this._running = new Map();
    }

    // Expands the command's leading token through the shell so the classifier
    // sees the real leaf binary, mirroring ShellCommandResolver.
    async classifyCommand(command) {
        const expanded = await this._expand(command);
        return classify(command, expanded);
    }

    async _expand(command) {
        const leading = command.split(' ')[0] ?? command;
        if (leading === '')
            return '';
        try {
            const result = await this._capture(shellArgv(
                `type -a ${leading} 2>/dev/null || which ${leading} 2>/dev/null`));
            return result.stdout;
        } catch {
            return '';
        }
    }

    _capture(argv) {
        return new Promise((resolve, reject) => {
            let proc;
            try {
                proc = Gio.Subprocess.new(argv,
                    Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
            } catch (e) {
                reject(e);
                return;
            }
            proc.communicate_utf8_async(null, null, (source, res) => {
                try {
                    const [, stdout, stderr] = source.communicate_utf8_finish(res);
                    resolve({stdout: stdout ?? '', stderr: stderr ?? ''});
                } catch (e) {
                    reject(e);
                }
            });
        });
    }

    // Hands the command to a real terminal, which has a TTY and starts an
    // interactive shell. The process lives in that terminal's session, so no
    // exit code is tracked — CommandStatus.launchedInTerminal says as much.
    launchInTerminal({command, accountId, trigger}) {
        const startedAtMs = Date.now();
        const terminal = findTerminal();
        if (!terminal) {
            const message = 'launch failed: no terminal emulator found on PATH';
            this._record({accountId, command, trigger, startedAtMs,
                status: STATUS.launchFailed, output: message});
            return {status: STATUS.launchFailed, outputTail: message};
        }
        try {
            GLib.spawn_async(null, terminalArgv(terminal, command), null,
                GLib.SpawnFlags.DEFAULT, null);
        } catch (e) {
            const message = `launch failed: ${e.message ?? e}`;
            this._record({accountId, command, trigger, startedAtMs,
                status: STATUS.launchFailed, output: message});
            return {status: STATUS.launchFailed, outputTail: message};
        }
        const note = `Launched in ${terminal.name}.`;
        this._record({accountId, command, trigger, startedAtMs,
            status: STATUS.launchedInTerminal, output: note});
        return {status: STATUS.launchedInTerminal, outputTail: note};
    }

    // Runs to completion, streaming output to `onOutput` and returning the
    // bounded tail. Resolves rather than rejects on every failure path, so a
    // caller never has to distinguish "the command failed" from "the runner
    // threw".
    run({command, accountId = null, trigger, onOutput = null}) {
        const startedAtMs = Date.now();
        return new Promise(resolve => {
            let proc;
            try {
                proc = Gio.Subprocess.new(shellArgv(command),
                    Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
            } catch (e) {
                const message = `launch failed: ${e.message ?? e}`;
                this._record({accountId, command, trigger, startedAtMs,
                    status: STATUS.launchFailed, output: message});
                resolve({status: STATUS.launchFailed, exitCode: null, outputTail: message});
                return;
            }

            const pid = parseInt(proc.get_identifier(), 10);
            const handle = {proc, pid, status: null};
            this._running.set(pid, handle);

            // The watchdog marks the status before signalling, so the finish
            // path below reports timedOut rather than a plain non-zero exit.
            const timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, this._timeoutSeconds, () => {
                if (handle.status === null) {
                    handle.status = STATUS.timedOut;
                    killTree(pid, 'TERM');
                    GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, KILL_GRACE_SECONDS, () => {
                        killTree(pid, 'KILL');
                        return GLib.SOURCE_REMOVE;
                    });
                }
                return GLib.SOURCE_REMOVE;
            });

            proc.communicate_utf8_async(null, null, (source, res) => {
                GLib.Source.remove(timeoutId);
                this._running.delete(pid);

                let output = '';
                try {
                    const [, stdout, stderr] = source.communicate_utf8_finish(res);
                    output = `${stdout ?? ''}${stderr ?? ''}`;
                } catch (e) {
                    output = `${e.message ?? e}`;
                }
                if (output !== '')
                    onOutput?.(output);

                const status = handle.status ?? STATUS.exited;
                const exitCode = status === STATUS.exited && proc.get_if_exited()
                    ? proc.get_exit_status()
                    : null;
                const outputTail = boundedTail(output);
                this._record({accountId, command, trigger, startedAtMs,
                    finishedAtMs: Date.now(), status, exitCode, output: outputTail});
                resolve({status, exitCode, outputTail});
            });
        });
    }

    // Kills one in-flight run, as dismissing the sheet does on macOS.
    cancel(pid) {
        const handle = this._running.get(pid);
        if (!handle)
            return;
        handle.status = STATUS.cancelled;
        killTree(pid, 'KILL');
    }

    // applicationWillTerminate's terminateAll: a Shell extension being disabled
    // must not leave its children running.
    terminateAll() {
        for (const pid of [...this._running.keys()])
            this.cancel(pid);
    }

    _record(row) {
        this._store?.record({finishedAtMs: Date.now(), exitCode: null, ...row});
    }
}

export {KIND};
