import SwiftUI

struct SettingsView: View {
	@ObservedObject var store: AccountStore

	var body: some View {
		Form {
			Section("Paths") {
				pathRow("Accounts", store.accountList?.accountsPath)
				pathRow("Config", store.accountList?.globalConfigPath)
			}

			Section("CLI") {
				Text(ProcessInfo.processInfo.environment["DECODEX_CLI"] ?? "decodex")
					.textSelection(.enabled)
			}
		}
		.formStyle(.grouped)
		.frame(width: 520, height: 220)
		.padding()
		.task {
			await store.refresh()
		}
	}

	private func pathRow(_ title: String, _ path: String?) -> some View {
		HStack {
			Text(title)
			Spacer()
			Text(path ?? "-")
				.foregroundStyle(.secondary)
				.lineLimit(1)
				.truncationMode(.middle)
				.textSelection(.enabled)
		}
	}
}
