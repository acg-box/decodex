import Foundation
import Observation

struct ResetCardAccountState: Identifiable, Equatable {
	let account: ResetCardAccountRecord
	let inventory: ResetCardInventory?
	let error: ResetCardClientError?
	let isRefreshing: Bool
	let profile: AccountProfileObservation?
	let profileUnavailable: AccountProfileUnavailable?
	let profileError: ResetCardClientError?
	let isProfileRefreshing: Bool

	init(
		account: ResetCardAccountRecord,
		inventory: ResetCardInventory?,
		error: ResetCardClientError?,
		isRefreshing: Bool,
		profile: AccountProfileObservation? = nil,
		profileUnavailable: AccountProfileUnavailable? = nil,
		profileError: ResetCardClientError? = nil,
		isProfileRefreshing: Bool = false
	) {
		self.account = account
		self.inventory = inventory
		self.error = error
		self.isRefreshing = isRefreshing
		self.profile = profile
		self.profileUnavailable = profileUnavailable
		self.profileError = profileError
		self.isProfileRefreshing = isProfileRefreshing
	}

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

	var isProfileDegraded: Bool {
		guard profile != nil else {
			return false
		}
		return profile?.isCached == true
			|| profileUnavailable != nil
			|| profileError != nil
	}

	var profileDegradationText: String? {
		guard profile != nil else {
			return nil
		}
		if let unavailable = profileUnavailable {
			return unavailable.error.presentation
		}
		if let profileError {
			return profileError.localizedDescription
		}
		if let refreshError = profile?.refreshError {
			return refreshError.presentation
		}
		return nil
	}
}

private struct AccountProfileRequest: Equatable, Sendable {
	let generation: UInt64
	let includesEmail: Bool
	let accountID: String
	let accountRevision: UInt64
}

private enum ResetCardAccountRead: Sendable {
	case inventoryAvailable(accountID: String, ResetCardInventory)
	case inventoryFailed(accountID: String, ResetCardClientError)
	case profileAvailable(
		accountID: String,
		request: AccountProfileRequest,
		AccountProfileObservation
	)
	case profileUnavailable(
		accountID: String,
		request: AccountProfileRequest,
		AccountProfileUnavailable
	)
	case profileFailed(
		accountID: String,
		request: AccountProfileRequest,
		ResetCardClientError
	)
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
	private(set) var routing: AccountRoutingControl?
	private(set) var isRefreshing = false
	private(set) var hasLoaded = false
	private(set) var submittingKey: String?
	private(set) var controllingAccountID: String?
	private(set) var isEnrollingAccount = false
	private(set) var isRoutingAccountControl = false
	private(set) var pendingAttempts: [ResetCardUseAttempt]
	private(set) var isPendingRecoveryBlocked: Bool
	private(set) var profileEmailsVisible = false
	var message: ResetCardStoreMessage?

	@ObservationIgnored private let client: any ResetCardClient
	@ObservationIgnored private let accountControlClient: (any AccountControlClient)?
	@ObservationIgnored private let accountProfileClient: (any AccountProfileClient)?
	@ObservationIgnored private let pendingStore: ResetCardPendingAttemptStore
	@ObservationIgnored private let startupRetryDelays: [Duration]
	@ObservationIgnored private var startupTask: Task<Void, Never>?
	@ObservationIgnored private var profileRequestGeneration: UInt64 = 0

	init(
		client: any ResetCardClient = ResetCardCLIClient(),
		pendingStore: ResetCardPendingAttemptStore = ResetCardPendingAttemptStore(),
		startupRetryDelays: [Duration] = ResetCardStore.defaultStartupRetryDelays
	) {
		self.client = client
		accountControlClient = client as? any AccountControlClient
		accountProfileClient = client as? any AccountProfileClient
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

	func setProfileEmailVisibility(_ isVisible: Bool) async {
		let didChange = profileEmailsVisible != isVisible
		profileEmailsVisible = isVisible
		if isVisible == false {
			invalidateProfileRequests()
			accounts = accounts.map { state in
				ResetCardAccountState(
					account: state.account,
					inventory: state.inventory,
					error: state.error,
					isRefreshing: state.isRefreshing,
					profile: state.profile?.redactingEmail(),
					profileUnavailable: state.profileUnavailable?.redactingEmail(),
					profileError: state.profileError,
					isProfileRefreshing: false
				)
			}
			return
		}

		guard didChange, accountProfileClient != nil else {
			return
		}
		await refreshProfiles()
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
			let discovered: [ResetCardAccountRecord]
			if let accountControlClient {
				let snapshot = try await accountControlClient.accountSnapshot(
					authority: retainedAuthority
				)
				routing = snapshot.routing
				discovered = snapshot.accounts
			} else {
				discovered = try await client.accounts(authority: retainedAuthority)
			}
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
				let retainedProfile = previous?.account.accountRevision == account.accountRevision
					? previous?.profile
					: nil
				return ResetCardAccountState(
					account: boundAccount,
					inventory: retainsInventory ? previous?.inventory : nil,
					error: nil,
					isRefreshing: true,
					profile: profileEmailsVisible
						? retainedProfile
						: retainedProfile?.redactingEmail(),
					profileUnavailable: nil,
					profileError: nil,
					isProfileRefreshing: accountProfileClient != nil
				)
			}
			if message?.tone == .error, isPendingRecoveryBlocked == false {
				message = nil
			}

			let client = self.client
			let accountProfileClient = self.accountProfileClient
			let includeEmail = profileEmailsVisible
			let profileGeneration = accountProfileClient.map { _ in
				beginProfileRequestGeneration()
			}
			var expectedAuthority = retainedAuthority
			await withTaskGroup(of: ResetCardAccountRead.self) { group in
				for state in accounts {
					let account = state.account
					group.addTask {
						do {
							return .inventoryAvailable(
								accountID: account.accountID,
								try await client.inventory(for: account)
							)
						} catch {
							return .inventoryFailed(
								accountID: account.accountID,
								Self.clientError(error)
							)
						}
					}
					if let accountProfileClient, let profileGeneration {
						let profileRequest = AccountProfileRequest(
							generation: profileGeneration,
							includesEmail: includeEmail,
							accountID: account.accountID,
							accountRevision: account.accountRevision
						)
						group.addTask {
							do {
								switch try await accountProfileClient.profile(
									for: account,
									includeEmail: includeEmail
								) {
								case .available(let profile):
									return .profileAvailable(
										accountID: account.accountID,
										request: profileRequest,
										profile
									)
								case .unavailable(let unavailable):
									return .profileUnavailable(
										accountID: account.accountID,
										request: profileRequest,
										unavailable
									)
								}
							} catch {
								return .profileFailed(
									accountID: account.accountID,
									request: profileRequest,
									Self.clientError(error)
								)
							}
						}
					}
				}

				for await read in group {
					guard Task.isCancelled == false else {
						return
					}
					switch read {
					case .inventoryAvailable(let accountID, let inventory):
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
					case .inventoryFailed(let accountID, let error):
						shouldRetry = shouldRetry || error.isRetryableReadFailure
						applyInventoryFailure(error, accountID: accountID)
					case .profileAvailable(let accountID, let request, let profile):
						guard isCurrentProfileRequest(request) else {
							continue
						}
						applyProfile(profile, accountID: accountID, request: request)
					case .profileUnavailable(let accountID, let request, let unavailable):
						guard isCurrentProfileRequest(request) else {
							continue
						}
						shouldRetry = shouldRetry || unavailable.error.isRetryableReadFailure
						applyProfileUnavailable(
							unavailable,
							accountID: accountID,
							request: request
						)
					case .profileFailed(let accountID, let request, let error):
						guard isCurrentProfileRequest(request) else {
							continue
						}
						shouldRetry = shouldRetry || error.isRetryableReadFailure
						applyProfileFailure(error, accountID: accountID, request: request)
					}
				}
			}
			if let profileGeneration {
				finishProfileRequestGeneration(profileGeneration)
			}
		} catch {
			let clientError = Self.clientError(error)
			shouldRetry = clientError.isRetryableReadFailure
			accounts = accounts.map {
				ResetCardAccountState(
					account: $0.account,
					inventory: $0.inventory,
					error: $0.error,
					isRefreshing: false,
					profile: $0.profile,
					profileUnavailable: $0.profileUnavailable,
					profileError: $0.profileError,
					isProfileRefreshing: false
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

	var isAccountControlInProgress: Bool {
		controllingAccountID != nil || isEnrollingAccount || isRoutingAccountControl
	}

	func isControllingAccount(_ accountID: String) -> Bool {
		controllingAccountID == accountID
	}

	func enrollFromSharedCodex(
		displayLabel: String,
		enabled: Bool = true
	) async {
		guard isRefreshing == false,
			isAccountControlInProgress == false,
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}

		isEnrollingAccount = true
		defer {
			isEnrollingAccount = false
		}
		let accountID = Self.newCanonicalUUID()
		let operationID = Self.newCanonicalUUID()
		let idempotencyKey = Self.newCanonicalUUID()
		await performAccountControl(
			isEnrollment: true,
			successMessage: "Account login imported.",
			operation: {
				try await accountControlClient.enrollFromSharedCodex(
					authority: establishedAuthority,
					operationID: operationID,
					accountID: accountID,
					displayLabel: displayLabel,
					enabled: enabled,
					idempotencyKey: idempotencyKey
				)
			}
		)
	}

	func renameAccount(
		_ accountID: String,
		displayLabel: String
	) async {
		guard let account = accountRecord(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		await performAccountControl(
			accountID: accountID,
			successMessage: "Account renamed.",
			operation: {
				try await accountControlClient.renameAccount(
					authority: account.authority ?? establishedAuthority,
					accountID: accountID,
					displayLabel: displayLabel,
					expectedRevision: account.accountRevision,
					idempotencyKey: Self.newCanonicalUUID()
				)
			}
		)
	}

	func setAccount(
		_ accountID: String,
		enabled: Bool
	) async {
		guard let account = accountRecord(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		await performAccountControl(
			accountID: accountID,
			successMessage: enabled ? "Account enabled." : "Account disabled.",
			operation: {
				try await accountControlClient.setAccountEnabled(
					authority: account.authority ?? establishedAuthority,
					accountID: accountID,
					enabled: enabled,
					expectedRevision: account.accountRevision,
					idempotencyKey: Self.newCanonicalUUID()
				)
			}
		)
	}

	func logoutAccount(_ accountID: String) async {
		guard let account = accountRecord(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		let operationID = Self.newCanonicalUUID()
		let idempotencyKey = Self.newCanonicalUUID()
		await performAccountControl(
			accountID: accountID,
			successMessage: "Account logged out.",
			operation: {
				try await accountControlClient.logoutAccount(
					authority: account.authority ?? establishedAuthority,
					operationID: operationID,
					accountID: accountID,
					expectedRevision: account.accountRevision,
					idempotencyKey: idempotencyKey
				)
			}
		)
	}

	func selectFixedAccount(_ accountID: String) async {
		guard let account = accountRecord(accountID),
			let routing,
			routing.order.contains(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		await performAccountControl(
			accountID: accountID,
			successMessage: "Fixed account selected.",
			operation: {
				try await accountControlClient.setFixedSelection(
					authority: account.authority ?? establishedAuthority,
					accountID: accountID,
					expectedAccountRevision: account.accountRevision,
					expectedRoutingRevision: routing.revision,
					idempotencyKey: Self.newCanonicalUUID()
				)
			}
		)
	}

	func selectBalancedAccounts() async {
		guard let routing,
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		await performAccountControl(
			successMessage: "Balanced account selection enabled.",
			operation: {
				try await accountControlClient.setBalancedSelection(
					authority: establishedAuthority,
					expectedRoutingRevision: routing.revision,
					idempotencyKey: Self.newCanonicalUUID()
				)
			}
		)
	}

	func refreshCredentials(for accountID: String) async {
		guard let account = accountRecord(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		let operationID = Self.newCanonicalUUID()
		let idempotencyKey = Self.newCanonicalUUID()
		await performAccountControl(
			accountID: accountID,
			successMessage: "Account credentials refreshed.",
			operation: {
				try await accountControlClient.refreshAccountCredentials(
					authority: account.authority ?? establishedAuthority,
					operationID: operationID,
					accountID: accountID,
					expectedRevision: account.accountRevision,
					idempotencyKey: idempotencyKey
				)
			}
		)
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
			credentialBinding: existing.credentialBinding,
			unsettledOperation: existing.unsettledOperation,
			fiveHourQuota: inventory.fiveHourQuota,
			sevenDayQuota: inventory.sevenDayQuota
		)
		let retainsProfileState = accounts[index].account.accountRevision
			== inventory.accountRevision
		accounts[index] = ResetCardAccountState(
			account: account,
			inventory: inventory,
			error: nil,
			isRefreshing: false,
			profile: retainsProfileState ? accounts[index].profile : nil,
			profileUnavailable: retainsProfileState
				? accounts[index].profileUnavailable
				: nil,
			profileError: retainsProfileState ? accounts[index].profileError : nil,
			isProfileRefreshing: accounts[index].isProfileRefreshing
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
			isRefreshing: false,
			profile: existing.profile,
			profileUnavailable: existing.profileUnavailable,
			profileError: existing.profileError,
			isProfileRefreshing: existing.isProfileRefreshing
		)
	}

	private func applyProfile(
		_ profile: AccountProfileObservation,
		accountID: String,
		request: AccountProfileRequest
	) {
		guard isCurrentProfileRequest(request),
			request.accountID == accountID,
			profile.accountID == accountID,
			let index = accounts.firstIndex(where: { $0.account.accountID == accountID }),
			profile.accountRevision == accounts[index].account.accountRevision
		else {
			applyProfileFailure(
				.invalidResponse,
				accountID: accountID,
				request: request
			)
			return
		}
		let existing = accounts[index]
		let candidate = profileEmailsVisible ? profile : profile.redactingEmail()
		guard Self.canReplaceRetainedProfile(
			existing.profile,
			with: candidate,
			allowsEmailEnrichment: profileEmailsVisible
		) else {
			accounts[index] = ResetCardAccountState(
				account: existing.account,
				inventory: existing.inventory,
				error: existing.error,
				isRefreshing: existing.isRefreshing,
				profile: existing.profile,
				profileUnavailable: nil,
				profileError: .invalidResponse,
				isProfileRefreshing: false
			)
			return
		}
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: existing.error,
			isRefreshing: existing.isRefreshing,
			profile: candidate,
			profileUnavailable: nil,
			profileError: nil,
			isProfileRefreshing: false
		)
	}

	private func applyProfileUnavailable(
		_ unavailable: AccountProfileUnavailable,
		accountID: String,
		request: AccountProfileRequest
	) {
		guard isCurrentProfileRequest(request),
			request.accountID == accountID,
			let index = accounts.firstIndex(where: { $0.account.accountID == accountID })
		else {
			return
		}
		let existing = accounts[index]
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: existing.error,
			isRefreshing: existing.isRefreshing,
			profile: existing.profile,
			profileUnavailable: profileEmailsVisible
				? unavailable
				: unavailable.redactingEmail(),
			profileError: nil,
			isProfileRefreshing: false
		)
	}

	private func applyProfileFailure(
		_ error: ResetCardClientError,
		accountID: String,
		request: AccountProfileRequest
	) {
		guard isCurrentProfileRequest(request),
			request.accountID == accountID,
			let index = accounts.firstIndex(where: { $0.account.accountID == accountID })
		else {
			return
		}
		let existing = accounts[index]
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: existing.error,
			isRefreshing: existing.isRefreshing,
			profile: existing.profile,
			profileUnavailable: nil,
			profileError: error,
			isProfileRefreshing: false
		)
	}

	private static func canReplaceRetainedProfile(
		_ existing: AccountProfileObservation?,
		with candidate: AccountProfileObservation,
		allowsEmailEnrichment: Bool
	) -> Bool {
		guard let existing,
			existing.accountRevision == candidate.accountRevision
		else {
			return true
		}
		if candidate.observedAtUnixMicros > existing.observedAtUnixMicros {
			return true
		}
		guard candidate.observedAtUnixMicros == existing.observedAtUnixMicros else {
			return false
		}
		if candidate == existing {
			return true
		}
		return allowsEmailEnrichment
			&& existing.email == nil
			&& candidate.email != nil
			&& existing.redactingEmail() == candidate.redactingEmail()
	}

	private func refreshProfiles() async {
		guard let accountProfileClient else {
			return
		}
		accounts = accounts.map { state in
			ResetCardAccountState(
				account: state.account,
				inventory: state.inventory,
				error: state.error,
				isRefreshing: state.isRefreshing,
				profile: state.profile,
				profileUnavailable: nil,
				profileError: nil,
				isProfileRefreshing: true
			)
		}
		let includeEmail = profileEmailsVisible
		let generation = beginProfileRequestGeneration()
		defer {
			finishProfileRequestGeneration(generation)
		}
		await withTaskGroup(of: ResetCardAccountRead.self) { group in
			for state in accounts {
				let account = state.account
				let request = AccountProfileRequest(
					generation: generation,
					includesEmail: includeEmail,
					accountID: account.accountID,
					accountRevision: account.accountRevision
				)
				group.addTask {
					do {
						switch try await accountProfileClient.profile(
							for: account,
							includeEmail: includeEmail
						) {
						case .available(let profile):
							return .profileAvailable(
								accountID: account.accountID,
								request: request,
								profile
							)
						case .unavailable(let unavailable):
							return .profileUnavailable(
								accountID: account.accountID,
								request: request,
								unavailable
							)
						}
					} catch {
						return .profileFailed(
							accountID: account.accountID,
							request: request,
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
				case .profileAvailable(let accountID, let responseRequest, let profile):
					guard isCurrentProfileRequest(responseRequest) else {
						continue
					}
					applyProfile(
						profile,
						accountID: accountID,
						request: responseRequest
					)
				case .profileUnavailable(let accountID, let responseRequest, let unavailable):
					guard isCurrentProfileRequest(responseRequest) else {
						continue
					}
					applyProfileUnavailable(
						unavailable,
						accountID: accountID,
						request: responseRequest
					)
				case .profileFailed(let accountID, let responseRequest, let error):
					guard isCurrentProfileRequest(responseRequest) else {
						continue
					}
					applyProfileFailure(
						error,
						accountID: accountID,
						request: responseRequest
					)
				case .inventoryAvailable, .inventoryFailed:
					break
				}
			}
		}
	}

	private func beginProfileRequestGeneration() -> UInt64 {
		profileRequestGeneration &+= 1
		return profileRequestGeneration
	}

	private func invalidateProfileRequests() {
		profileRequestGeneration &+= 1
	}

	private func isCurrentProfileRequest(_ request: AccountProfileRequest) -> Bool {
		guard request.generation == profileRequestGeneration,
			request.includesEmail == profileEmailsVisible,
			let state = accounts.first(where: {
				$0.account.accountID == request.accountID
			})
		else {
			return false
		}
		return state.account.accountRevision == request.accountRevision
	}

	private func finishProfileRequestGeneration(_ generation: UInt64) {
		guard generation == profileRequestGeneration else {
			return
		}
		accounts = accounts.map { state in
			guard state.isProfileRefreshing else {
				return state
			}
			return ResetCardAccountState(
				account: state.account,
				inventory: state.inventory,
				error: state.error,
				isRefreshing: state.isRefreshing,
				profile: state.profile,
				profileUnavailable: state.profileUnavailable,
				profileError: state.profileError,
				isProfileRefreshing: false
			)
		}
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
			credentialBinding: account.credentialBinding,
			unsettledOperation: account.unsettledOperation,
			fiveHourQuota: account.fiveHourQuota,
			sevenDayQuota: account.sevenDayQuota
		)
	}

	nonisolated private static func clientError(_ error: Error) -> ResetCardClientError {
		error as? ResetCardClientError ?? .invalidResponse
	}

	private var establishedAuthority: ResetCardAuthority? {
		let authorities = Set(
			accounts.compactMap { $0.account.authority ?? $0.inventory?.authority }
		)
		return authorities.count == 1 ? authorities.first : nil
	}

	private func accountRecord(_ accountID: String) -> ResetCardAccountRecord? {
		accounts.first(where: { $0.account.accountID == accountID })?.account
	}

	private func performAccountControl(
		accountID: String? = nil,
		isEnrollment: Bool = false,
		successMessage: String,
		operation: () async throws -> AccountControlResult
	) async {
		guard isRefreshing == false,
			controllingAccountID == nil,
			isRoutingAccountControl == false
		else {
			presentAccountControlUnavailable()
			return
		}
		if isEnrollment {
			guard isEnrollingAccount else {
				presentAccountControlUnavailable()
				return
			}
		} else {
			guard isEnrollingAccount == false else {
				presentAccountControlUnavailable()
				return
			}
		}

		if let accountID {
			controllingAccountID = accountID
		} else if isEnrollment == false {
			isRoutingAccountControl = true
		}
		defer {
			if accountID != nil {
				controllingAccountID = nil
			} else if isEnrollment == false {
				isRoutingAccountControl = false
			}
		}

		let actionMessage: ResetCardStoreMessage
		do {
			_ = try await operation()
			actionMessage = ResetCardStoreMessage(tone: .success, text: successMessage)
		} catch let error as AccountControlError {
			actionMessage = ResetCardStoreMessage(
				tone: .error,
				text: error.localizedDescription
			)
		} catch let error as ResetCardClientError {
			actionMessage = ResetCardStoreMessage(
				tone: .error,
				text: error.localizedDescription
			)
		} catch {
			actionMessage = ResetCardStoreMessage(
				tone: .error,
				text: AccountControlError.invalidResponse.localizedDescription
			)
		}

		_ = await refreshReadState()
		message = actionMessage
	}

	private func presentAccountControlUnavailable() {
		message = ResetCardStoreMessage(
			tone: .error,
			text: "Account controls are unavailable while another account request is active."
		)
	}

	nonisolated private static func newCanonicalUUID() -> String {
		UUID().uuidString.lowercased()
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

private extension AccountProfileObservationError {
	var isRetryableReadFailure: Bool {
		switch self {
		case .productStateUnavailable, .credentialUnavailable,
			.providerUnavailable, .accountChanged:
			return true
		case .invalidRequest, .accountUnavailable, .unauthorized,
			.protocolUnavailable:
			return false
		}
	}
}
