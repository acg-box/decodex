import SwiftUI

struct LoginSheetView: View {
	@ObservedObject var store: AccountStore
	@Environment(\.dismiss) private var dismiss

	var body: some View {
		VStack(alignment: .leading, spacing: 14) {
			HStack(spacing: 10) {
				Image(systemName: "person.badge.key")
					.font(.title2)
				VStack(alignment: .leading, spacing: 2) {
					Text("Add Codex Login")
						.font(.headline)
					Text("A temporary Codex home is used, then the resulting auth file is imported.")
						.font(.caption)
						.foregroundStyle(.secondary)
				}
			}

			ScrollView {
				Text(store.loginTranscript.isEmpty ? "Ready to start device login." : store.loginTranscript)
					.font(.system(.caption, design: .monospaced))
					.foregroundStyle(.secondary)
					.frame(maxWidth: .infinity, alignment: .leading)
					.textSelection(.enabled)
					.padding(10)
			}
			.frame(height: 220)
			.background(.quaternary, in: RoundedRectangle(cornerRadius: 8, style: .continuous))

			HStack {
				Button("Cancel") {
					dismiss()
				}
				.keyboardShortcut(.cancelAction)

				Spacer()

				Button {
					Task {
						await store.login()
						if store.notice == nil {
							dismiss()
						}
					}
				} label: {
					Label(store.isLoggingIn ? "Logging In" : "Start Login", systemImage: "arrow.right.circle")
				}
				.keyboardShortcut(.defaultAction)
				.disabled(store.isLoggingIn)
			}
		}
		.frame(width: 500)
		.padding(18)
	}
}
