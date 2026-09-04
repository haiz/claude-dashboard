import SwiftUI

/// Adds or repairs an account from a session key pasted by hand.
///
/// Separate from `SetupView` on purpose. The scan path treats "already added" as
/// a failure; here a match is the success case, because repairing the key on a
/// stored account is the main thing this screen is for.
struct PasteKeyView: View {
    @ObservedObject var viewModel: DashboardViewModel
    let onClose: () -> Void

    @State private var key: String = ""
    @State private var isWorking = false
    @State private var outcome: ManualKeyOutcome?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Paste a session key")
                .font(.headline)

            Text("Open claude.ai in your browser, copy the value of the `sessionKey` cookie, and paste it here. Use this when the app cannot read the cookie from the browser itself.")
                .font(.caption)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)

            SecureField("sessionKey", text: $key)
                .textFieldStyle(.roundedBorder)
                .disabled(isWorking)

            if let outcome {
                Text(outcome.message)
                    .font(.caption)
                    .foregroundStyle(outcome.isFailure ? .red : .secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack {
                Button(outcome?.holdsSheetOpen == true ? "Done" : "Cancel", action: onClose)
                    .disabled(isWorking)
                Spacer()
                Button(isWorking ? "Checking\u{2026}" : "Add") {
                    Task { await submit() }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(isWorking || key.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 420)
    }

    private func submit() async {
        isWorking = true
        let result = await viewModel.applyManualKey(key)
        isWorking = false
        outcome = result
        guard !result.isFailure else { return }
        key = ""
        guard !result.holdsSheetOpen else { return }
        onClose()
    }
}
