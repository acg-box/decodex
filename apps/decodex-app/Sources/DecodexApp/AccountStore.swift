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
	var notice: AccountNotice?
	var loginNotice: AccountNotice?
	var pendingLogoutRemovalKeys = Set<String>()
	var usageRefillAnimations: [String: AccountUsageRefillAnimation] = [:]

	@ObservationIgnored let bridge = DecodexAppBridge()
	@ObservationIgnored var startupTask: Task<Void, Never>?
	@ObservationIgnored var automaticRefreshTask: Task<Void, Never>?
	@ObservationIgnored var operatorSnapshotStreamTask: Task<Void, Never>?
	@ObservationIgnored var operatorSnapshotPublishedAtUnixEpoch: Int64?
	@ObservationIgnored var usageRefillCleanupTasks: [String: Task<Void, Never>] = [:]
	@ObservationIgnored var noticeDismissalTask: Task<Void, Never>?

	deinit {
		startupTask?.cancel()
		automaticRefreshTask?.cancel()
		operatorSnapshotStreamTask?.cancel()
		noticeDismissalTask?.cancel()
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
		if loginNotice?.tone == .error {
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
		clearLoginNotice()
	}

	func presentNotice(_ notice: AccountNotice) {
		guard notice.scope == .general else {
			loginNotice = notice
			return
		}

		if notice.tone == .error,
			self.notice?.hasSamePresentation(as: notice) == true
		{
			return
		}

		noticeDismissalTask?.cancel()
		self.notice = notice

		guard let delay = notice.automaticDismissalDelay else {
			noticeDismissalTask = nil
			return
		}

		let noticeID = notice.id
		noticeDismissalTask = Task { [weak self] in
			do {
				try await Task.sleep(for: delay)
			} catch {
				return
			}

			guard let self, self.notice?.id == noticeID else {
				return
			}

			self.notice = nil
			self.noticeDismissalTask = nil
		}
	}

	func presentError(
		_ summary: String,
		error: Error,
		scope: AccountNotice.Scope = .general,
		source: AccountNotice.Source = .accountAction
	) {
		presentNotice(.error(
			summary,
			details: error.localizedDescription,
			scope: scope,
			source: source
		))
	}

	func clearNotice() {
		dismissCurrentNotice()
	}

	func clearNotice(source: AccountNotice.Source) {
		guard notice?.source == source else {
			return
		}

		dismissCurrentNotice()
	}

	private func dismissCurrentNotice() {
		noticeDismissalTask?.cancel()
		noticeDismissalTask = nil
		notice = nil
	}

	func clearLoginNotice() {
		loginNotice = nil
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
