import AppKit
import Foundation
import OSLog

private let accountStoreLog = Logger(subsystem: "ink.hack.DecodexApp", category: "AccountStore")
private let accountUsageRefreshIntervalNanoseconds: UInt64 = 15_000_000_000
private let operatorSnapshotReconnectInitialDelay: UInt64 = 1_000_000_000
private let operatorSnapshotReconnectMaxDelay: UInt64 = 30_000_000_000

@MainActor
final class AccountStore: ObservableObject {
	@Published private(set) var accountList: AccountListResponse?
	@Published private(set) var fastMode: CodexFastModeResponse?
	@Published private(set) var operatorSnapshot: OperatorSnapshotResponse?
	@Published private(set) var operatorPresentation: OperatorSnapshotPresentation?
	@Published private(set) var operatorSnapshotUpdatedAt: Date?
	@Published private(set) var isRefreshing = false
	@Published private(set) var isLoggingIn = false
	@Published private(set) var isSettingFastMode = false
	@Published var loginTranscript = ""
	@Published var notice: String?
	@Published private var pendingLogoutRemovalKeys = Set<String>()

	private let bridge = DecodexAppBridge()
	private var automaticRefreshTask: Task<Void, Never>?
	private var operatorSnapshotStreamTask: Task<Void, Never>?
	private var operatorSnapshotPublishedAtUnixEpoch: Int64?

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

	func refresh(force: Bool = false) async {
		guard isRefreshing == false else {
			return
		}

		isRefreshing = true
		defer {
			isRefreshing = false
		}

		do {
			applyAccountList(try await bridge.runJSON(
				.accountList(forceRefresh: force),
				as: AccountListResponse.self
			))
			notice = nil
			await refreshFastMode()
		} catch {
			notice = error.localizedDescription
		}
	}

	func refreshIfNeeded() async {
		guard accountList == nil else {
			return
		}

		await refresh(force: true)
	}

	func openWebUI() async {
		do {
			let url = try await DecodexServerBridge.shared.dashboardURL()

			NSWorkspace.shared.open(url)
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func resetLoginSession() {
		guard isLoggingIn == false else {
			return
		}

		loginTranscript = ""
		notice = nil
	}

	func startAutomaticRefresh() {
		guard automaticRefreshTask == nil else {
			return
		}

		automaticRefreshTask = Task { [weak self] in
			while Task.isCancelled == false {
				do {
					try await Task.sleep(nanoseconds: accountUsageRefreshIntervalNanoseconds)
				} catch {
					return
				}

				await self?.refresh(force: true)
			}
		}

		startOperatorSnapshotStream()
	}

	func startOperatorSnapshotStream() {
		guard operatorSnapshotStreamTask == nil else {
			return
		}

		operatorSnapshotStreamTask = makeOperatorSnapshotStreamTask()
	}

	private func makeOperatorSnapshotStreamTask() -> Task<Void, Never> {
		Task { [weak self] in
			await self?.runOperatorSnapshotStream()
		}
	}

	private func runOperatorSnapshotStream() async {
		var reconnectDelay = operatorSnapshotReconnectInitialDelay

		while Task.isCancelled == false {
			do {
				try await connectOperatorSnapshotStream()
				reconnectDelay = operatorSnapshotReconnectInitialDelay
			} catch {
				accountStoreLog.warning("Operator snapshot stream dropped: \(error.localizedDescription, privacy: .public)")
			}

			do {
				try await Task.sleep(nanoseconds: reconnectDelay)
			} catch {
				return
			}
			reconnectDelay = min(operatorSnapshotReconnectMaxDelay, reconnectDelay * 2)
		}
	}

	private func connectOperatorSnapshotStream() async throws {
		let url = try await DecodexServerBridge.shared.dashboardWebSocketURL()
		let socket = DashboardWebSocketConnection(url: url)

		try await withTaskCancellationHandler {
			do {
				try await socket.connect()
				while Task.isCancelled == false {
					let data = try await socket.readMessageData()
					do {
						let event = try JSONDecoder().decode(OperatorDashboardSocketEvent.self, from: data)
						applyOperatorDashboardEvent(event)
					} catch {
						accountStoreLog.debug("Skipped dashboard WebSocket message bytes=\(data.count, privacy: .public) error=\(error.localizedDescription, privacy: .public)")
						continue
					}
				}
				await socket.close()
			} catch {
				await socket.close()
				throw error
			}
		} onCancel: {
			Task {
				await socket.close()
			}
		}
	}

	func applyOperatorDashboardEvent(_ event: OperatorDashboardSocketEvent) {
		guard let payload = event.payload else {
			return
		}

		switch event.type {
		case "snapshot":
			guard let snapshot = payload.snapshot else {
				return
			}

			operatorSnapshot = snapshot
			operatorPresentation = snapshot.presentation
			operatorSnapshotPublishedAtUnixEpoch = payload.snapshotPublishedAtUnixEpoch
			operatorSnapshotUpdatedAt = payload.snapshotPublishedAt ?? Date()
		case "runActivity":
			guard let presentation = payload.presentation else {
				return
			}
			guard isStaleRunActivity(payload) == false else {
				return
			}

			operatorPresentation = presentation
			operatorSnapshotUpdatedAt = payload.emittedAt ?? Date()
		default:
			break
		}
	}

	private func isStaleRunActivity(_ payload: OperatorDashboardSocketPayload) -> Bool {
		guard let emittedAtUnixEpoch = payload.emittedAtUnixEpoch,
			let snapshotPublishedAtUnixEpoch = operatorSnapshotPublishedAtUnixEpoch
		else {
			return false
		}

		return emittedAtUnixEpoch < snapshotPublishedAtUnixEpoch
	}

	func useInCodex(_ account: CodexAccount) async {
		let previousAccountList = accountList
		notice = nil
		accountList = accountList?.updatingCodexAuth(account.authIdentity)

		do {
			let response = try await bridge.runJSON(
				.accountUse(selector: account.selector),
				as: CodexAuthUseResponse.self
			)
			accountList = accountList?.updatingCodexAuth(response.account)
			notice = nil
		} catch {
			accountList = previousAccountList
			notice = error.localizedDescription
		}
	}

	func select(_ account: CodexAccount) async {
		do {
			if account.selected {
				applyAccountList(try await bridge.runJSON(.accountClear, as: AccountListResponse.self))
			} else {
				applyAccountList(try await bridge.runJSON(
					.accountSelect(selector: account.selector),
					as: AccountListResponse.self
				))
			}
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func clearSelection() async {
		do {
			applyAccountList(try await bridge.runJSON(.accountClear, as: AccountListResponse.self))
			notice = nil
		} catch {
			notice = error.localizedDescription
		}
	}

	func setFastMode(_ enabled: Bool) async {
		guard isSettingFastMode == false else {
			return
		}

		let previous = fastMode
		fastMode = CodexFastModeResponse(
			codexConfigPath: previous?.codexConfigPath ?? "",
			enabled: enabled
		)
		isSettingFastMode = true
		defer {
			isSettingFastMode = false
		}

		do {
			fastMode = try await bridge.runJSON(
				.codexFastModeSet(enabled: enabled),
				as: CodexFastModeResponse.self
			)
			notice = nil
		} catch {
			fastMode = previous
			notice = error.localizedDescription
		}
	}

	func logout(_ account: CodexAccount) async throws {
		beginOptimisticLogoutRemoval(account)

		do {
			applyAccountList(try await bridge.runJSON(
				.accountLogout(selector: account.selector),
				as: AccountListResponse.self
			))
			notice = nil
		} catch {
			cancelOptimisticLogoutRemoval(account)
			throw error
		}
	}

	func login() async {
		isLoggingIn = true
		loginTranscript = ""
		notice = nil

		do {
			applyAccountList(try await bridge.runStreaming(.accountLogin(), as: AccountListResponse.self) { [weak self] chunk in
				self?.loginTranscript += chunk
			})
			notice = nil
			await refreshFastMode()
		} catch {
			notice = error.localizedDescription
		}

		isLoggingIn = false
	}

	private func refreshFastMode() async {
		do {
			fastMode = try await bridge.runJSON(
				.codexFastModeStatus,
				as: CodexFastModeResponse.self
			)
		} catch {
			notice = error.localizedDescription
		}
	}

	func beginOptimisticLogoutRemoval(_ account: CodexAccount) {
		pendingLogoutRemovalKeys.formUnion(Self.logoutRemovalKeys(for: account))
	}

	func cancelOptimisticLogoutRemoval(_ account: CodexAccount) {
		pendingLogoutRemovalKeys.subtract(Self.logoutRemovalKeys(for: account))
	}

	func applyAccountList(_ response: AccountListResponse) {
		accountList = response
		reconcilePendingLogoutRemovals(with: response.accounts)
	}

	private func reconcilePendingLogoutRemovals(with accounts: [CodexAccount]) {
		guard pendingLogoutRemovalKeys.isEmpty == false else {
			return
		}

		let visibleKeys = accounts.reduce(into: Set<String>()) { keys, account in
			keys.formUnion(Self.logoutRemovalKeys(for: account))
		}
		pendingLogoutRemovalKeys = pendingLogoutRemovalKeys.intersection(visibleKeys)
	}

	private func isLogoutRemovalPending(for account: CodexAccount) -> Bool {
		Self.logoutRemovalKeys(for: account).isDisjoint(with: pendingLogoutRemovalKeys) == false
	}

	private static func logoutRemovalKeys(for account: CodexAccount) -> Set<String> {
		[
			account.id,
			account.selector,
			account.email,
			account.accountFingerprint,
		]
		.compactMap { value in
			guard let key = value?.trimmingCharacters(in: .whitespacesAndNewlines),
				key.isEmpty == false
			else {
				return nil
			}
			return key
		}
		.reduce(into: Set<String>()) { keys, key in
			keys.insert(key)
		}
	}
}

private extension AccountListResponse {
	func updatingCodexAuth(_ identity: CodexAuthIdentity) -> AccountListResponse {
		AccountListResponse(
			accountsPath: accountsPath,
			globalConfigPath: globalConfigPath,
			codexAuthPath: codexAuthPath,
			codexAuth: identity,
			control: control,
			accounts: accounts.map { account in
				account.withCodexActive(account.matchesSelector(identity.selector))
			},
			usageEstimate: usageEstimate,
			usageProbeError: usageProbeError
		)
	}
}

struct DeviceLoginPrompt: Equatable {
	let verificationURL: URL?
	let userCode: String

	var compactCode: String {
		userCode.filter { character in
			character.isLetter || character.isNumber
		}
	}

	static func parse(_ transcript: String) -> DeviceLoginPrompt? {
		let text = stripANSI(transcript)
		guard let code = parseUserCode(from: text) else {
			return nil
		}

		return DeviceLoginPrompt(
			verificationURL: parseVerificationURL(from: text),
			userCode: code
		)
	}

	private static func parseVerificationURL(from text: String) -> URL? {
		let pattern = #"https?://[^\s\)>\]]+"#
		guard
			let expression = try? NSRegularExpression(pattern: pattern),
			let match = expression.firstMatch(
				in: text,
				range: NSRange(text.startIndex..<text.endIndex, in: text)
			),
			let range = Range(match.range, in: text)
		else {
			return nil
		}

		let value = String(text[range]).trimmingCharacters(in: CharacterSet(charactersIn: ".,;"))
		return URL(string: value)
	}

	private static func parseUserCode(from text: String) -> String? {
		let lines = text.components(separatedBy: .newlines)
		if let codeLine = lineAfterCodePrompt(in: lines) {
			return normalizedCode(from: codeLine)
		}

		for line in lines.reversed() {
			if let code = normalizedCode(from: line) {
				return code
			}
		}

		return nil
	}

	private static func lineAfterCodePrompt(in lines: [String]) -> String? {
		for (index, line) in lines.enumerated() where line.localizedCaseInsensitiveContains("one-time code") {
			for candidate in lines.dropFirst(index + 1) {
				if normalizedCode(from: candidate) != nil {
					return candidate
				}
			}
		}

		return nil
	}

	private static func normalizedCode(from line: String) -> String? {
		let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
		guard trimmed.isEmpty == false else {
			return nil
		}

		let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789- ")
		guard trimmed.uppercased().unicodeScalars.allSatisfy({ allowed.contains($0) }) else {
			return nil
		}

		let compact = trimmed.uppercased().filter { character in
			character.isLetter || character.isNumber
		}
		guard (6...12).contains(compact.count) else {
			return nil
		}

		if compact.count > 4 {
			let split = compact.index(compact.startIndex, offsetBy: 4)
			return "\(compact[..<split])-\(compact[split...])"
		}

		return compact
	}

	private static func stripANSI(_ value: String) -> String {
		guard let expression = try? NSRegularExpression(pattern: "\u{001B}\\[[0-9;]*[A-Za-z]") else {
			return value
		}

		let range = NSRange(value.startIndex..<value.endIndex, in: value)
		return expression.stringByReplacingMatches(
			in: value,
			range: range,
			withTemplate: ""
		)
	}
}
