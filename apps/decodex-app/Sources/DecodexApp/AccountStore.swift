import Foundation

@MainActor
final class AccountStore: ObservableObject {
	@Published var accountList: AccountListResponse?
	@Published var fastMode: CodexFastModeResponse?
	@Published var operatorSnapshot: OperatorSnapshotResponse?
	@Published var operatorPresentation: OperatorSnapshotPresentation?
	@Published var operatorSnapshotUpdatedAt: Date?
	@Published var isRefreshing = false
	@Published var isLoggingIn = false
	@Published var isSettingFastMode = false
	@Published var loginTranscript = ""
	@Published var notice: String?
	@Published var pendingLogoutRemovalKeys = Set<String>()

	let bridge = DecodexAppBridge()
	var automaticRefreshTask: Task<Void, Never>?
	var operatorSnapshotStreamTask: Task<Void, Never>?
	var operatorSnapshotPublishedAtUnixEpoch: Int64?

	deinit {
		automaticRefreshTask?.cancel()
		operatorSnapshotStreamTask?.cancel()
	}

	var isInitialLoading: Bool {
		accountList == nil && isRefreshing
	}

	var accounts: [CodexAccount] {
		guard let accounts = accountList?.accounts else {
			return []
		}

		return accounts.filter { isLogoutRemovalPending(for: $0) == false }
	}

	var fastModeEnabled: Bool {
		fastMode?.enabled == true
	}

	var modeLabel: String {
		guard let control = accountList?.control else {
			return "Not loaded"
		}

		let codexLabel = accountList?.codexAuth?.displayName ?? "no Codex auth"
		if let selector = control.accountSelector, selector.isEmpty == false {
			return "Codex: \(codexLabel) / Decodex: \(selector)"
		}

		let decodexLabel = control.mode == "balanced" ? "balanced" : control.mode
		return "Codex: \(codexLabel) / Decodex: \(decodexLabel)"
	}

	var loginPrompt: DeviceLoginPrompt? {
		DeviceLoginPrompt.parse(loginTranscript)
	}

	var loginStatusLabel: String {
		if isLoggingIn {
			return loginPrompt == nil ? "Requesting code" : "Waiting for browser sign-in"
		}
		if notice != nil {
			return "Login failed"
		}
		if loginPrompt != nil {
			return "Code ready"
		}
		if loginTranscript.isEmpty {
			return "Ready"
		}

			return "Importing account"
		}

	func resetLoginSession() {
		guard isLoggingIn == false else {
			return
		}

		loginTranscript = ""
		notice = nil
	}
}

struct CodexFastModeResponse: Decodable, Equatable {
	let codexConfigPath: String
	let enabled: Bool

	enum CodingKeys: String, CodingKey {
		case codexConfigPath = "codex_config_path"
		case enabled
	}
}
