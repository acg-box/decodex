import Foundation
import Observation

struct ResetCardAccountState: Identifiable, Equatable {
	let account: ResetCardAccountRecord
	let inventory: ResetCardInventory?
	let error: ResetCardClientError?
	let isRefreshing: Bool

	var id: String {
		account.accountID
	}

	var fiveHourQuota: ResetCardQuotaWindow {
		inventory?.fiveHourQuota ?? account.fiveHourQuota
	}

	var sevenDayQuota: ResetCardQuotaWindow {
		inventory?.sevenDayQuota ?? account.sevenDayQuota
	}

	var targets: [ResetCardUseTarget] {
		guard let inventory else {
			return []
		}

		return inventory.cards.map {
			ResetCardUseTarget(
				authority: inventory.authority,
				accountID: inventory.accountID,
				expectedRevision: inventory.accountRevision,
				descriptor: $0
			)
		}
	}
}

private enum ResetCardInventoryRead: Sendable {
	case available(accountID: String, ResetCardInventory)
	case failed(accountID: String, ResetCardClientError)
}

struct ResetCardStoreMessage: Equatable {
	enum Tone: Equatable {
		case information
		case success
		case error
	}

	let tone: Tone
	let text: String
}

private enum ResetCardDispatchOutcome {
	case state(ResetCardOperationState)
	case error(ResetCardClientError)

	var removesPendingAttempt: Bool {
		switch self {
		case .state(.completed), .state(.failedBeforeEffect),
			.error(.commandRejected):
			return true
		case .state(.prepared), .state(.effectAmbiguous), .state(.notFound),
			.state(.unavailable), .error:
			return false
		}
	}
}

private enum ResetCardRefreshResult: Equatable {
	case complete
	case retryNeeded
	case skipped
}

@MainActor
@Observable
final class ResetCardStore {
	private static let defaultStartupRetryDelays: [Duration] = [
		.seconds(1),
		.seconds(2),
		.seconds(4),
		.seconds(8),
		.seconds(15),
		.seconds(30),
	]

	private(set) var accounts = [ResetCardAccountState]()
	private(set) var isRefreshing = false
	private(set) var hasLoaded = false
	private(set) var submittingKey: String?
	private(set) var pendingAttempts: [ResetCardUseAttempt]
	private(set) var isPendingRecoveryBlocked: Bool
	var message: ResetCardStoreMessage?

	@ObservationIgnored private let client: any ResetCardClient
	@ObservationIgnored private let pendingStore: ResetCardPendingAttemptStore
	@ObservationIgnored private let startupRetryDelays: [Duration]
	@ObservationIgnored private var startupTask: Task<Void, Never>?

	init(
		client: any ResetCardClient = ResetCardCLIClient(),
		pendingStore: ResetCardPendingAttemptStore = ResetCardPendingAttemptStore(),
		startupRetryDelays: [Duration] = ResetCardStore.defaultStartupRetryDelays
	) {
		self.client = client
		self.pendingStore = pendingStore
		self.startupRetryDelays = startupRetryDelays
		let pendingLoad = pendingStore.load()
		pendingAttempts = pendingLoad.attempts
		isPendingRecoveryBlocked = pendingLoad.isRecoveryBlocked
		if isPendingRecoveryBlocked {
			message = Self.pendingRecoveryBlockedMessage
		}
	}

	deinit {
		startupTask?.cancel()
	}

	var isInitialLoading: Bool {
		isRefreshing && hasLoaded == false
	}

	func blocksNewAttempt(for target: ResetCardUseTarget) -> Bool {
		isPendingRecoveryBlocked
			|| submittingKey != nil
			|| pendingAttempts.count >= ResetCardPendingAttemptStore.maximumAttempts
			|| pendingAttempts.contains(where: {
				$0.target.accountID == target.accountID
					&& $0.target.descriptor == target.descriptor
			})
	}

	func start() {
		guard startupTask == nil else {
			return
		}

		let retryDelays = startupRetryDelays
		startupTask = Task { [weak self] in
			// Keep automatic retries on account, inventory, and pending-status reads.
			// Only an explicit user action can reach the reset-card use command.
			guard var result = await self?.refreshReadState() else {
				return
			}

			for delay in retryDelays {
				guard result == .retryNeeded, Task.isCancelled == false else {
					return
				}

				do {
					try await Task.sleep(for: delay)
				} catch {
					return
				}

				guard let refreshed = await self?.refreshReadState() else {
					return
				}
				result = refreshed
			}
		}
	}

	func refresh() async {
		_ = await refreshReadState()
	}

	private func refreshReadState() async -> ResetCardRefreshResult {
		guard isRefreshing == false else {
			return .skipped
		}

		isRefreshing = true
		defer {
			isRefreshing = false
			hasLoaded = true
		}

		var shouldRetry = false
		do {
			let retainedAuthorities = Set(
				accounts.compactMap { $0.account.authority ?? $0.inventory?.authority }
			)
			guard retainedAuthorities.count <= 1 else {
				throw ResetCardClientError.invalidResponse
			}
			let retainedAuthority = retainedAuthorities.first
			let discovered = try await client.accounts(authority: retainedAuthority)
			let previousByID = Dictionary(
				uniqueKeysWithValues: accounts.map { ($0.account.accountID, $0) }
			)
			accounts = discovered.map { account in
				let previous = previousByID[account.accountID]
				let authority = retainedAuthority
					?? previous?.account.authority
					?? previous?.inventory?.authority
				let boundAccount = Self.account(account, authority: authority)
				let retainsInventory = previous?.account.accountRevision == account.accountRevision
				return ResetCardAccountState(
					account: boundAccount,
					inventory: retainsInventory ? previous?.inventory : nil,
					error: nil,
					isRefreshing: true
				)
			}
			if message?.tone == .error, isPendingRecoveryBlocked == false {
				message = nil
			}

			let client = self.client
			var expectedAuthority = retainedAuthority
			await withTaskGroup(of: ResetCardInventoryRead.self) { group in
				for state in accounts {
					let account = state.account
					group.addTask {
						do {
							return .available(
								accountID: account.accountID,
								try await client.inventory(for: account)
							)
						} catch {
							return .failed(
								accountID: account.accountID,
								Self.clientError(error)
							)
						}
					}
				}

				for await read in group {
					guard Task.isCancelled == false else {
						return
					}
					switch read {
					case .available(let accountID, let inventory):
						guard expectedAuthority == nil || expectedAuthority == inventory.authority else {
							applyInventoryFailure(
								.invalidResponse,
								accountID: accountID
							)
							continue
						}
						expectedAuthority = inventory.authority
						if let error = inventory.observationError {
							shouldRetry = shouldRetry || error.isRetryableReadFailure
						}
						applyInventory(inventory, accountID: accountID)
					case .failed(let accountID, let error):
						shouldRetry = shouldRetry || error.isRetryableReadFailure
						applyInventoryFailure(error, accountID: accountID)
					}
				}
			}
		} catch {
			let clientError = Self.clientError(error)
			shouldRetry = clientError.isRetryableReadFailure
			accounts = accounts.map {
				ResetCardAccountState(
					account: $0.account,
					inventory: $0.inventory,
					error: $0.error,
					isRefreshing: false
				)
			}
			message = ResetCardStoreMessage(
				tone: .error,
				text: clientError.localizedDescription
			)
		}

		shouldRetry = await recoverPendingAttempts() || shouldRetry
		if isPendingRecoveryBlocked {
			message = Self.pendingRecoveryBlockedMessage
		}

		return shouldRetry ? .retryNeeded : .complete
	}

	func use(_ attempt: ResetCardUseAttempt) async -> ResetCardUseCompletion {
		guard isPendingRecoveryBlocked == false else {
			message = Self.pendingRecoveryBlockedMessage
			return ResetCardUseCompletion(resolved: true)
		}
		guard submittingKey == nil,
			let current = accounts.first(where: { $0.account.accountID == attempt.target.accountID }),
			current.targets.contains(attempt.target)
		else {
			message = ResetCardStoreMessage(
				tone: .error,
				text: "The reset cards changed. Refresh and select the card again."
			)
			return ResetCardUseCompletion(resolved: true)
		}
		guard pendingAttempts.contains(where: {
			$0.target.accountID == attempt.target.accountID
				&& $0.target.descriptor == attempt.target.descriptor
		}) == false
		else {
			message = ResetCardStoreMessage(
				tone: .information,
				text: "Resume the pending request for this reset card with its existing operation key."
			)
			return ResetCardUseCompletion(resolved: true)
		}
		guard remember(attempt) else {
			message = ResetCardStoreMessage(
				tone: .error,
				text: "The pending reset-card limit is reached. Resolve an existing request before starting another."
			)
			return ResetCardUseCompletion(resolved: true)
		}

		return await submit(attempt)
	}

	func resume(_ attempt: ResetCardUseAttempt) async {
		guard submittingKey == nil,
			pendingAttempts.contains(attempt)
		else {
			return
		}

		submittingKey = attempt.idempotencyKey
		defer {
			submittingKey = nil
		}

		if isPendingRecoveryBlocked {
			do {
				let observed = try await client.status(for: attempt)
				if observed != .notFound {
					_ = await apply(observed, to: attempt)
				}
				message = Self.pendingRecoveryBlockedMessage
			} catch {
				_ = await apply(Self.clientError(error), to: attempt)
				message = Self.pendingRecoveryBlockedMessage
			}
			return
		}

		guard let dispatch = await pendingStore.withDispatchLock(
			for: attempt,
			operation: {
				do {
					let observed = try await client.status(for: attempt)
					let state = observed == .notFound
						? try await client.use(attempt)
						: observed
					return ResetCardDispatchOutcome.state(state)
				} catch {
					return ResetCardDispatchOutcome.error(Self.clientError(error))
				}
			},
			shouldRemove: \.removesPendingAttempt
		) else {
			reloadPendingJournal()
			message = isPendingRecoveryBlocked
				? Self.pendingRecoveryBlockedMessage
				: Self.pendingDispatchUnavailableMessage
			return
		}
		_ = await apply(dispatch, to: attempt)
	}

	func dismissMessage() {
		message = nil
	}

	func accountLabel(for accountID: String) -> String {
		accounts.first(where: { $0.account.accountID == accountID })?
			.account.displayLabel
			?? "Account …\(accountID.suffix(8))"
	}

	private func submit(_ attempt: ResetCardUseAttempt) async -> ResetCardUseCompletion {
		submittingKey = attempt.idempotencyKey
		defer {
			submittingKey = nil
		}

		guard let dispatch = await pendingStore.withDispatchLock(
			for: attempt,
			operation: {
				do {
					return ResetCardDispatchOutcome.state(
						try await client.use(attempt)
					)
				} catch {
					return ResetCardDispatchOutcome.error(Self.clientError(error))
				}
			},
			shouldRemove: \.removesPendingAttempt
		) else {
			reloadPendingJournal()
			message = isPendingRecoveryBlocked
				? Self.pendingRecoveryBlockedMessage
				: Self.pendingDispatchUnavailableMessage
			return ResetCardUseCompletion(resolved: false)
		}

		return await apply(dispatch, to: attempt)
	}

	private func apply(
		_ dispatch: ResetCardPendingDispatchResult<ResetCardDispatchOutcome>,
		to attempt: ResetCardUseAttempt
	) async -> ResetCardUseCompletion {
		reloadPendingJournal()
		guard dispatch.journalUpdate != .removalFailed else {
			message = Self.pendingTerminalRemovalFailedMessage
			return ResetCardUseCompletion(resolved: false)
		}

		switch dispatch.value {
		case .state(let state):
			return await apply(
				state,
				to: attempt,
				removeTerminalAttempt: false
			)
		case .error(let error):
			return await apply(
				error,
				to: attempt,
				removeTerminalAttempt: false
			)
		}
	}

	private func apply(
		_ error: ResetCardClientError,
		to attempt: ResetCardUseAttempt,
		removeTerminalAttempt: Bool = true
	) async -> ResetCardUseCompletion {
		message = ResetCardStoreMessage(
			tone: .error,
			text: error.localizedDescription
		)

		switch error {
		case .commandRejected:
			if removeTerminalAttempt {
				forget(attempt)
			}
			await refreshAccount(attempt.target.accountID)
			return ResetCardUseCompletion(resolved: true)
		case .executableMissing, .launchFailed, .timedOut, .outputTooLarge,
			.useDefinitelyNotDispatched, .usePotentiallyDispatched,
			.commandFailed, .invalidResponse, .service:
			reloadPendingJournal()
			return ResetCardUseCompletion(resolved: false)
		}
	}

	private func apply(
		_ state: ResetCardOperationState,
		to attempt: ResetCardUseAttempt,
		removeTerminalAttempt: Bool = true
	) async -> ResetCardUseCompletion {
		message = ResetCardStoreMessage(
			tone: Self.messageTone(for: state),
			text: state.presentation
		)

		switch state {
		case .completed, .failedBeforeEffect:
			if removeTerminalAttempt {
				forget(attempt)
			}
			await refreshAccount(attempt.target.accountID)
			return ResetCardUseCompletion(resolved: true)
		case .prepared, .effectAmbiguous, .notFound, .unavailable:
			reloadPendingJournal()
			return ResetCardUseCompletion(resolved: false)
		}
	}

	private func recoverPendingAttempts() async -> Bool {
		guard submittingKey == nil else {
			return false
		}

		var shouldRetry = false
		for attempt in pendingAttempts {
			guard Task.isCancelled == false else {
				return false
			}

			do {
				let state = try await client.status(for: attempt)
				switch state {
				case .completed, .failedBeforeEffect:
					_ = await apply(state, to: attempt)
				case .prepared, .effectAmbiguous:
					shouldRetry = true
					message = ResetCardStoreMessage(
						tone: Self.messageTone(for: state),
						text: state.presentation
					)
				case .unavailable(let error):
					shouldRetry = shouldRetry || error.isRetryableReadFailure
					message = ResetCardStoreMessage(
						tone: Self.messageTone(for: state),
						text: state.presentation
					)
				case .notFound:
					message = ResetCardStoreMessage(
						tone: .information,
						text: "A pending reset-card request can be resumed with the same operation key."
					)
				}
			} catch {
				let clientError = Self.clientError(error)
				shouldRetry = shouldRetry || clientError.isRetryableReadFailure
				message = ResetCardStoreMessage(
					tone: .error,
					text: clientError.localizedDescription
				)
			}
		}
		if isPendingRecoveryBlocked {
			message = Self.pendingRecoveryBlockedMessage
		}

		return shouldRetry
	}

	@discardableResult
	private func remember(_ attempt: ResetCardUseAttempt) -> Bool {
		guard isPendingRecoveryBlocked == false else {
			return false
		}
		guard let updated = pendingStore.insert(attempt) else {
			return false
		}
		pendingAttempts = updated
		return true
	}

	private func forget(_ attempt: ResetCardUseAttempt) {
		guard isPendingRecoveryBlocked == false else {
			return
		}
		if let updated = pendingStore.remove(attempt) {
			pendingAttempts = updated
		}
	}

	private func reloadPendingJournal() {
		let load = pendingStore.load()
		pendingAttempts = load.attempts
		isPendingRecoveryBlocked = load.isRecoveryBlocked
	}

	private func refreshAccount(_ accountID: String) async {
		guard let index = accounts.firstIndex(where: { $0.account.accountID == accountID }) else {
			return
		}

		let existing = accounts[index]
		do {
			let inventory = try await client.inventory(for: existing.account)
			applyInventory(inventory, accountID: accountID)
		} catch {
			applyInventoryFailure(Self.clientError(error), accountID: accountID)
		}
	}

	private func applyInventory(
		_ inventory: ResetCardInventory,
		accountID: String
	) {
		guard inventory.accountID == accountID,
			let index = accounts.firstIndex(where: { $0.account.accountID == accountID })
		else {
			applyInventoryFailure(.invalidResponse, accountID: accountID)
			return
		}
		let existing = accounts[index].account
		let account = ResetCardAccountRecord(
			authority: inventory.authority,
			accountID: existing.accountID,
			displayLabel: existing.displayLabel,
			accountRevision: inventory.accountRevision,
			enabled: existing.enabled,
			observedState: existing.observedState,
			lifecycleReadiness: existing.lifecycleReadiness,
			fiveHourQuota: inventory.fiveHourQuota,
			sevenDayQuota: inventory.sevenDayQuota
		)
		accounts[index] = ResetCardAccountState(
			account: account,
			inventory: inventory,
			error: nil,
			isRefreshing: false
		)
	}

	private func applyInventoryFailure(
		_ error: ResetCardClientError,
		accountID: String
	) {
		guard let index = accounts.firstIndex(where: { $0.account.accountID == accountID }) else {
			return
		}
		let existing = accounts[index]
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: error,
			isRefreshing: false
		)
	}

	private static func account(
		_ account: ResetCardAccountRecord,
		authority: ResetCardAuthority?
	) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: authority,
			accountID: account.accountID,
			displayLabel: account.displayLabel,
			accountRevision: account.accountRevision,
			enabled: account.enabled,
			observedState: account.observedState,
			lifecycleReadiness: account.lifecycleReadiness,
			fiveHourQuota: account.fiveHourQuota,
			sevenDayQuota: account.sevenDayQuota
		)
	}

	nonisolated private static func clientError(_ error: Error) -> ResetCardClientError {
		error as? ResetCardClientError ?? .invalidResponse
	}

	private static func messageTone(for state: ResetCardOperationState) -> ResetCardStoreMessage.Tone {
		switch state {
		case .completed(.reset):
			return .success
		case .completed, .prepared, .effectAmbiguous, .notFound, .unavailable:
			return .information
		case .failedBeforeEffect:
			return .error
		}
	}

	private static let pendingRecoveryBlockedMessage = ResetCardStoreMessage(
		tone: .error,
		text: "The pending reset-card recovery journal is invalid or unavailable. New use is blocked. Preserve the journal for manual inspection; no automatic repair is available."
	)

	private static let pendingDispatchUnavailableMessage = ResetCardStoreMessage(
		tone: .information,
		text: "Another app instance changed or is using this pending reset-card request. Refresh before continuing."
	)

	private static let pendingTerminalRemovalFailedMessage = ResetCardStoreMessage(
		tone: .error,
		text: "The reset-card operation is terminal, but the recovery journal could not be updated. Preserve the journal and resume this request before starting another."
	)
}

private extension ResetCardClientError {
	var isRetryableReadFailure: Bool {
		switch self {
		case .timedOut, .commandFailed:
			return true
		case .service(let error):
			return error.isRetryableReadFailure
		case .executableMissing, .launchFailed, .outputTooLarge, .commandRejected,
			.useDefinitelyNotDispatched, .usePotentiallyDispatched, .invalidResponse:
			return false
		}
	}
}

private extension ResetCardServiceError {
	var isRetryableReadFailure: Bool {
		switch self {
		case .accountNotFound, .accountStateRejected, .vaultUnavailable, .providerUnavailable,
			.inventoryIncomplete, .inventoryChanged, .resourceExhausted,
			.productStateUnavailable, .effectAmbiguous:
			return true
		case .invalidRequest, .schemaUnsupported:
			return false
		}
	}
}
