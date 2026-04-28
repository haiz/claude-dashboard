import SwiftUI

struct RunCommandSheet: View {
    let account: Account
    @Binding var isPresented: Bool
    let onRefresh: () -> Void

    @State private var command: String
    @State private var isRunning = false
    @State private var logLines: [String] = []
    @FocusState private var isFocused: Bool

    private var userDefaultsKey: String { "runCommand_\(account.id.uuidString)" }

    init(account: Account, isPresented: Binding<Bool>, onRefresh: @escaping () -> Void) {
        self.account = account
        self._isPresented = isPresented
        self.onRefresh = onRefresh
        let saved = UserDefaults.standard.string(forKey: "runCommand_\(account.id.uuidString)") ?? ""
        self._command = State(initialValue: saved)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Run Command")
                .font(.headline)

            TextField("Enter command...", text: $command)
                .textFieldStyle(.roundedBorder)
                .focused($isFocused)
                .onSubmit { if !command.isEmpty && !isRunning { run() } }

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
        .onAppear { isFocused = true }
    }

    private func run() {
        UserDefaults.standard.set(command, forKey: userDefaultsKey)
        isRunning = true
        logLines = []
        let cmd = command

        Task.detached {
            let process = Process()
            process.launchPath = "/bin/zsh"
            process.arguments = ["-c", cmd]
            process.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())

            let outPipe = Pipe()
            let errPipe = Pipe()
            process.standardOutput = outPipe
            process.standardError = errPipe

            let handleData: @Sendable (Data) -> Void = { data in
                guard !data.isEmpty,
                      let s = String(data: data, encoding: .utf8) else { return }
                let lines = s.components(separatedBy: .newlines).filter { !$0.isEmpty }
                guard !lines.isEmpty else { return }
                DispatchQueue.main.async { logLines.append(contentsOf: lines) }
            }

            outPipe.fileHandleForReading.readabilityHandler = { handleData($0.availableData) }
            errPipe.fileHandleForReading.readabilityHandler = { handleData($0.availableData) }

            try? process.run()
            process.waitUntilExit()

            outPipe.fileHandleForReading.readabilityHandler = nil
            errPipe.fileHandleForReading.readabilityHandler = nil

            // Drain any remaining buffered output
            let tail = outPipe.fileHandleForReading.readDataToEndOfFile()
            let errTail = errPipe.fileHandleForReading.readDataToEndOfFile()
            handleData(tail)
            handleData(errTail)

            try? await Task.sleep(nanoseconds: 2_000_000_000)

            await MainActor.run {
                isPresented = false
                onRefresh()
            }
        }
    }
}
