import SwiftUI

/// An alert-style card drawn inside its parent, over a dimmed backdrop.
struct InlinePrompt: View {
    let title: String
    let message: String
    let button: String
    /// Tooltip for `button`.
    let buttonHelp: String
    /// Non-nil adds a Cancel button beside `button`.
    var onCancel: (() -> Void)? = nil
    let action: () -> Void

    var body: some View {
        ZStack {
            Color.black.opacity(0.35)
            VStack(alignment: .leading, spacing: 10) {
                Text(title).font(.headline)
                Text(message).fixedSize(horizontal: false, vertical: true)
                HStack {
                    if let onCancel {
                        Button(action: onCancel) {
                            Text("Cancel").frame(maxWidth: .infinity)
                        }
                        .keyboardShortcut(.cancelAction)
                        .help("Close without continuing")
                    }
                    Button(action: action) {
                        Text(button).frame(maxWidth: .infinity)
                    }
                    .keyboardShortcut(.defaultAction)
                    .help(buttonHelp)
                }
                .controlSize(.large)
                .padding(.top, 4)
            }
            .padding(16)
            .frame(maxWidth: 260)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
            .shadow(radius: 12)
        }
    }
}
