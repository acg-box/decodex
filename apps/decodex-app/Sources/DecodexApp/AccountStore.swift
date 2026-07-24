import Foundation
import Observation

@MainActor
@Observable
final class AccountStore {
	var accountList: AccountListResponse?
	var fastMode: CodexFastModeResponse?
	var operatorSnapshot: OperatorSnapshotResponse?
	var operatorPresentation: OperatorSnapshotPresentation?
	var operatorSnapshotUpdatedAt: Date?
	var isRefreshing = false
	var isLoggingIn = false
	var isSettingFastMode = false
	var loginTranscript = ""
	var notice: String?
	var pendingLogoutRemovalKeys = Set<String>()
	var usageRefillAnimations: [String: AccountUsageRefillAnimation] = [:]

	@ObservationIgnored let bridge = DecodexAppBridge()
	@ObservationIgnored var startupTask: Task<Void, Never>?
	@ObservationIgnored var automaticRefreshTask: Task<Void, Never>?
	@ObservationIgnored var operatorSnapshotStreamTask: Task<Void, Never>?
	@ObservationIgnored var operatorSnapshotPublishedAtUnixEpoch: Int64?
	@ObservationIgnored var usageRefillCleanupTasks: [String: Task<Void, Never>] = [:]

	deinit {
		startupTask?.cancel()
		automaticRefreshTask?.cancel()
		operatorSnapshotStreamTask?.cancel()
		for task in usageRefillCleanupTasks.values {
			task.cancel()
		}
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
