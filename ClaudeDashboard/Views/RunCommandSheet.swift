import SwiftUI

struct RunCommandSheet: View {
    let account: Account
    @Binding var isPresented: Bool
    let onRefresh: () -> Void
    let runner: CommandRunner

    @State private var command: String
    @State private var isRunning = false
    @State private var openInTerminal: Bool
    @State private var userTouchedToggle = false
    @State private var logLines: [String] = []
    @State private var runTask: Task<Void, Never>?
    @FocusState private var isFocused: Bool

    private let classifier = CommandClassifier(resolver: ShellCommandResolver())

    private var commandKey: String { "runCommand_\(account.id.uuidString)" }
    private var terminalKey: String { "runCommandTerminal_\(account.id.uuidString)" }

    init(account: Account, isPresented: Binding<Bool>, runner: CommandRunner, onRefresh: @escaping () -> Void) {
        self.account = account
        self._isPresented = isPresented
        self.runner = runner
        self.onRefresh = onRefresh
        let saved = UserDefaults.standard.string(forKey: "runCommand_\(account.id.uuidString)") ?? ""
        self._command = State(initialValue: saved)
        self._openInTerminal = State(initialValue: UserDefaults.standard.bool(forKey: "runCommandTerminal_\(account.id.uuidString)"))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Run Command")
                .font(.headline)

            TextField("Enter command...", text: $command)
                .textFieldStyle(.roundedBorder)
                .focused($isFocused)
                .onSubmit { if !command.isEmpty && !isRunning { run() } }
                .onChange(of: command) { _ in
                    userTouchedToggle = false
                    detectMode()
                }

            Toggle("Open in Terminal", isOn: $openInTerminal)
                .toggleStyle(.checkbox)
                .font(.caption)
                .onChange(of: openInTerminal) { _ in userTouchedToggle = true }

            if !logLines.isEmpty {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(logLines.suffix(2).enumerated()), id: \.offset) { _, line in
                        Text(line)
                            .font(.system(.caption, design: .monospaced))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(.horizontal, 4)
            }

            HStack {
                Spacer()
                Button("Cancel") {
                    runTask?.cancel()
                    isPresented = false
                }
                .keyboardShortcut(.escape, modifiers: [])

                Button(isRunning ? "Running..." : "Run") {
                    run()
                }
                .disabled(command.isEmpty || isRunning)
            }
        }
        .padding(20)
        .frame(minWidth: 300)
        .onAppear { isFocused = true; detectMode() }
    }

    /// Async-classify the current command and, unless the user already flipped the
    /// toggle, set the default mode. Guarded against races on stale command text.
    private func detectMode() {
        let cmd = command
        guard !cmd.isEmpty else { return }
        Task {
            let kind = await classifier.classify(cmd)
            await MainActor.run {
                guard !userTouchedToggle, cmd == command else { return }
                openInTerminal = (kind == .interactive)
            }
        }
    }

    private func run() {
        UserDefaults.standard.set(command, forKey: commandKey)
        UserDefaults.standard.set(openInTerminal, forKey: terminalKey)
        let cmd = command
        let acct = account.id
        let runner = self.runner

        if openInTerminal {
            Task {
                await runner.launchInTerminal(command: cmd, accountId: acct, trigger: .manual)
                await MainActor.run { isPresented = false; onRefresh() }
            }
            return
        }

        isRunning = true
        logLines = []
        runTask = Task {
            await runner.run(command: cmd, accountId: acct, trigger: .manual) { chunk in
                let lines = chunk.components(separatedBy: .newlines).filter { !$0.isEmpty }
                guard !lines.isEmpty else { return }
                DispatchQueue.main.async { logLines.append(contentsOf: lines) }
            }
            await MainActor.run {
                isRunning = false
                isPresented = false
                onRefresh()
            }
        }
    }
}
