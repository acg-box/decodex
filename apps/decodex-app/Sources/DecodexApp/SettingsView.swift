import SwiftUI

struct SettingsView: View {
	@ObservedObject var store: AccountStore

	var body: some View {
		Form {
			Section("Paths") {
				pathRow("Accounts", store.accountList?.accountsPath)
				pathRow("Config", store.accountList?.globalConfigPath)
				pathRow("Codex Auth", store.accountList?.codexAuthPath)
			}

			Section("Helper") {
				Text(ProcessInfo.processInfo.environment["DECODEX_APP_HELPER"] ?? "Bundled decodex-app-helper")
					.textSelection(.enabled)
			}

			Section("Server") {
				Text(ProcessInfo.processInfo.environment["DECODEX_APP_SERVER_URL"] ?? "http://127.0.0.1:8912")
					.textSelection(.enabled)
				Text(ProcessInfo.processInfo.environment["DECODEX_APP_DECODEX"] ?? "Bundled decodex")
					.textSelection(.enabled)
			}
		}
		.formStyle(.grouped)
		.frame(width: 520, height: 320)
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
