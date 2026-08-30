// ClaudeDashboard/Services/CommandClassifier.swift
import Foundation

enum CommandKind { case interactive, nonInteractive }

/// Expands a command's leading token through the shell (following functions/aliases)
/// so the classifier can see the real leaf binary. Injectable for tests.
protocol CommandResolver: Sendable {
    func expand(_ command: String) async -> String
}

/// Decides whether a command opens an interactive terminal program (needs a TTY) or
/// runs to completion. The entered text is often an opaque shell-function name, so the
/// resolver expands it first. Result is only a default; the user override wins.
struct CommandClassifier {
    let resolver: CommandResolver

    func classify(_ command: String) async -> CommandKind {
        let expanded = await resolver.expand(command)
        return Self.classify(command: command, expanded: expanded)
    }

    /// Pure classification. `command` is the raw text the user entered (may contain
    /// quoted argument prose); `expanded` is the shell's resolution of its leading token
    /// (a real path, or a function body naming its target binary).
    static func classify(command: String, expanded: String) -> CommandKind {
        // Name matching must never see argument prose (e.g. `echo "no more files"`), so
        // the haystack is the resolved expansion plus only the command's leading token.
        let nameHaystack = (firstToken(of: command) + "\n" + expanded).lowercased()

        func word(_ w: String) -> Bool {
            nameHaystack.range(of: "\\b\(NSRegularExpression.escapedPattern(for: w))\\b",
                                options: .regularExpression) != nil
        }

        // claude: interactive unless print/non-interactive output mode is requested.
        if word("claude") {
            return claudePrintMode(command: command, expanded: expanded) ? .nonInteractive : .interactive
        }

        // Editors, pagers, monitors, multiplexers, REPLs: interactive TUIs.
        let interactiveTools = ["vim", "vi", "nvim", "nano", "emacs",
                                "top", "htop", "btop", "less", "more",
                                "tmux", "irb", "lazygit", "fzf"]
        for tool in interactiveTools where word(tool) { return .interactive }

        // ssh: interactive shell unless a remote command is supplied.
        if word("ssh") { return sshInteractive(command) ? .interactive : .nonInteractive }

        return .nonInteractive
    }

    private static func firstToken(of command: String) -> String {
        command.split(separator: " ").first.map(String.init) ?? command
    }

    /// True when `command`/`expanded` request claude's non-interactive print mode.
    /// Quoted argument text is stripped from `command` first so prose like
    /// `claude "explain the -p flag"` cannot be mistaken for the real `-p` flag.
    private static func claudePrintMode(command: String, expanded: String) -> Bool {
        let tokens = stripQuotedRegions(command).split(whereSeparator: { $0.isWhitespace })
        let hasPrintToken = tokens.contains { $0 == "-p" || $0 == "--print" || $0.hasPrefix("--output-format") }
        let expandedLower = expanded.lowercased()
        let expandedHasPrint = expandedLower.contains("--print") || expandedLower.contains("--output-format")
        return hasPrintToken || expandedHasPrint
    }

    /// Removes `"..."` and `'...'` spans (quotes included), replacing each with a single
    /// space so surrounding tokens don't get glued together.
    private static func stripQuotedRegions(_ s: String) -> String {
        var result = ""
        let chars = Array(s)
        var i = 0
        while i < chars.count {
            let c = chars[i]
            if c == "\"" || c == "'" {
                let quote = c
                i += 1
                while i < chars.count && chars[i] != quote { i += 1 }
                if i < chars.count { i += 1 } // consume closing quote if present
                result.append(" ")
            } else {
                result.append(c)
                i += 1
            }
        }
        return result
    }

    /// Short flags that consume the following token as their value (never a positional).
    private static let sshValueFlags: Set<String> = [
        "-p", "-i", "-l", "-o", "-F", "-L", "-R", "-D", "-b", "-c", "-E", "-J", "-m", "-Q", "-w"
    ]

    /// `ssh host` -> interactive; `ssh host cmd...` -> runs a remote command, non-interactive.
    private static func sshInteractive(_ command: String) -> Bool {
        var tokens = command.split(separator: " ").map(String.init)
        guard let idx = tokens.firstIndex(of: "ssh") else { return true }
        tokens.removeFirst(idx + 1)

        // Walk tokens, dropping option flags and (for value-taking flags) their argument,
        // keeping only genuine positionals.
        var positionals: [String] = []
        var i = 0
        while i < tokens.count {
            let tok = tokens[i]
            if tok.hasPrefix("-") {
                i += sshValueFlags.contains(tok) ? 2 : 1
                continue
            }
            positionals.append(tok)
            i += 1
        }
        // First positional is the host. Anything beyond it is a remote command.
        return positionals.count <= 1
    }
}

/// Resolves via an interactive zsh so ~/.zshrc functions/aliases are defined. One level
/// of `whence` output is enough for the common case (a function body naming its target
/// binary); nested indirection falls back to the user override.
struct ShellCommandResolver: CommandResolver {
    /// Leading tokens must look like a bare command name/path. Anything else (shell
    /// metacharacters such as `$()`, backticks, `;`, `|`, `&`) is refused outright so it
    /// is never interpolated into (and executed by) the resolution script.
    private static let allowedLeadingToken = "^[A-Za-z0-9._/-]+$"

    func expand(_ command: String) async -> String {
        let first = command.split(separator: " ").first.map(String.init) ?? command
        guard first.range(of: Self.allowedLeadingToken, options: .regularExpression) != nil else {
            return ""
        }
        let script = "whence -c \(first) 2>/dev/null; whence -v \(first) 2>/dev/null"
        return await Self.runInteractiveZsh(script)
    }

    /// Thread-safe accumulator shared between the readability handler (background I/O
    /// queue), the termination handler, and the timeout fallback. Guarantees the
    /// continuation resumes exactly once no matter which of those fires first.
    private final class ResolverState: @unchecked Sendable {
        private let lock = NSLock()
        private var buffer = Data()
        private var finished = false

        func append(_ data: Data) {
            lock.lock(); defer { lock.unlock() }
            guard !finished else { return }
            buffer.append(data)
        }

        /// Returns true exactly once (on the first caller) so only one code path resumes
        /// the continuation.
        func finishOnce() -> Bool {
            lock.lock(); defer { lock.unlock() }
            if finished { return false }
            finished = true
            return true
        }

        func snapshot() -> Data {
            lock.lock(); defer { lock.unlock() }
            return buffer
        }
    }

    private static func runInteractiveZsh(_ script: String) async -> String {
        await withCheckedContinuation { (cont: CheckedContinuation<String, Never>) in
            let p = Process()
            p.launchPath = "/bin/zsh"
            p.arguments = ["-ic", script]
            let pipe = Pipe()
            p.standardOutput = pipe
            // No pipe for stderr: ~/.zshrc (nvm/oh-my-zsh/etc.) can print a lot on an
            // interactive shell, and an undrained stderr pipe fills its 64KB buffer and
            // blocks the child before it ever reaches our script.
            p.standardError = FileHandle.nullDevice

            let state = ResolverState()

            // Drain stdout continuously while the process runs so it never blocks on a
            // full pipe waiting for a reader that only shows up at termination.
            pipe.fileHandleForReading.readabilityHandler = { handle in
                let chunk = handle.availableData
                guard !chunk.isEmpty else { return }
                state.append(chunk)
            }

            func finish() {
                pipe.fileHandleForReading.readabilityHandler = nil
                cont.resume(returning: String(data: state.snapshot(), encoding: .utf8) ?? "")
            }

            p.terminationHandler = { _ in
                if state.finishOnce() { finish() }
            }

            do {
                try p.run()
            } catch {
                if state.finishOnce() { finish() }
                return
            }

            // Safety net: if the interactive shell never terminates (e.g. a hung rc
            // file), stop waiting on it. Terminate, escalate to SIGKILL after a brief
            // grace period, then resume with whatever output was collected so far.
            DispatchQueue.global().asyncAfter(deadline: .now() + 5) {
                guard p.isRunning else { return }
                p.terminate()
                DispatchQueue.global().asyncAfter(deadline: .now() + 0.5) {
                    if p.isRunning {
                        kill(p.processIdentifier, SIGKILL)
                    }
                    if state.finishOnce() { finish() }
                }
            }
        }
    }
}
