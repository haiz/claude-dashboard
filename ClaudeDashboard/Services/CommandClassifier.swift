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

    /// Pure classification over the original command plus its shell expansion.
    static func classify(command: String, expanded: String) -> CommandKind {
        let hay = (command + "\n" + expanded).lowercased()

        func word(_ w: String) -> Bool {
            hay.range(of: "\\b\(NSRegularExpression.escapedPattern(for: w))\\b",
                      options: .regularExpression) != nil
        }

        // claude: interactive unless print/non-interactive output mode is requested.
        if word("claude") || hay.contains("claude-awake") {
            let printMode = hay.contains(" -p ") || hay.hasSuffix(" -p")
                || hay.contains("--print") || hay.contains("--output-format")
            return printMode ? .nonInteractive : .interactive
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

    /// `ssh host` -> interactive; `ssh host cmd...` -> runs a remote command, non-interactive.
    private static func sshInteractive(_ command: String) -> Bool {
        var tokens = command.split(separator: " ").map(String.init)
        guard let idx = tokens.firstIndex(of: "ssh") else { return true }
        tokens.removeFirst(idx + 1)
        // Drop option flags and their obvious argument forms; keep positional tokens.
        let positionals = tokens.filter { !$0.hasPrefix("-") }
        // First positional is the host. Anything beyond it is a remote command.
        return positionals.count <= 1
    }
}

/// Resolves via an interactive zsh so ~/.zshrc functions/aliases are defined. One level
/// of `whence` output is enough for the common case (a function body naming its target
/// binary); nested indirection falls back to the user override.
struct ShellCommandResolver: CommandResolver {
    func expand(_ command: String) async -> String {
        let first = command.split(separator: " ").first.map(String.init) ?? command
        let safe = first.replacingOccurrences(of: "'", with: "")
        let script = "whence -c \(safe) 2>/dev/null; whence -v \(safe) 2>/dev/null"
        return await Self.runInteractiveZsh(script)
    }

    private static func runInteractiveZsh(_ script: String) async -> String {
        await withCheckedContinuation { (cont: CheckedContinuation<String, Never>) in
            let p = Process()
            p.launchPath = "/bin/zsh"
            p.arguments = ["-ic", script]
            let pipe = Pipe()
            p.standardOutput = pipe
            p.standardError = Pipe()
            p.terminationHandler = { _ in
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                cont.resume(returning: String(data: data, encoding: .utf8) ?? "")
            }
            do { try p.run() } catch { cont.resume(returning: "") }
        }
    }
}
