import Foundation
import Observation

struct ResetCardAccountState: Identifiable, Equatable {
	let account: ResetCardAccountRecord
	let inventory: ResetCardInventory?
	let error: ResetCardClientError?

	var id: String {
		account.accountID
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
			let discovered = try await client.accounts()
			var refreshed = [ResetCardAccountState]()
			refreshed.reserveCapacity(discovered.count)

			for account in discovered {
				guard Task.isCancelled == false else {
					return .complete
				}

				do {
					let inventory = try await client.inventory(for: account)
					let currentAccount = ResetCardAccountRecord(
						authority: inventory.authority,
						accountID: account.accountID,
						displayLabel: account.displayLabel,
						accountRevision: inventory.accountRevision,
						admissionState: account.admissionState
					)
					refreshed.append(
						ResetCardAccountState(
							account: currentAccount,
							inventory: inventory,
							error: nil
						)
					)
				} catch {
					let clientError = Self.clientError(error)
					shouldRetry = shouldRetry || clientError.isRetryableReadFailure
					refreshed.append(
						ResetCardAccountState(
							account: account,
							inventory: nil,
							error: clientError
						)
					)
				}
			}

			accounts = refreshed
			if message?.tone == .error, isPendingRecoveryBlocked == false {
				message = nil
			}
		} catch {
			let clientError = Self.clientError(error)
			shouldRetry = clientError.isRetryableReadFailure
			accounts = []
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
			let account = ResetCardAccountRecord(
				authority: inventory.authority,
				accountID: existing.account.accountID,
				displayLabel: existing.account.displayLabel,
				accountRevision: inventory.accountRevision,
				admissionState: existing.account.admissionState
			)
			accounts[index] = ResetCardAccountState(
				account: account,
				inventory: inventory,
				error: nil
			)
		} catch {
			accounts[index] = ResetCardAccountState(
				account: existing.account,
				inventory: nil,
				error: Self.clientError(error)
			)
		}
	}

	private static func clientError(_ error: Error) -> ResetCardClientError {
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
