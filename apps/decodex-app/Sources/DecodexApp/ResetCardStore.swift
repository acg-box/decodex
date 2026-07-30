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

	var requiresLoginRefresh: Bool {
		account.observedState == .authFailed
	}
}

private struct AccountProfileRequest: Equatable, Sendable {
	let generation: UInt64
	let privacyEpoch: UInt64
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

enum AccountControlActivity: Equatable {
	case lifecycle
	case loginRefresh
	case codexProjection
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
	// Profile reads are cheap cached projections, while Reset Card reads start an
	// account-bound provider process. Keep the progressive fan-out below the host
	// process burst that can make otherwise healthy accounts fail transiently.
	private static let maximumConcurrentAccountReads = 3

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
	private(set) var codexAuthProjection: CodexAuthProjection?
	private(set) var isRefreshing = false
	private(set) var refreshSkeletonIsPublished = false
	private(set) var isRefreshingAccountSkeleton = false
	private(set) var hasLoaded = false
	private(set) var submittingKey: String?
	private(set) var accountControlActivities = [String: AccountControlActivity]()
	private(set) var isEnrollingAccount = false
	private(set) var isRoutingAccountControl = false
	private(set) var accountSkeletonRevisionTargets = [String: UInt64]()
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
	@ObservationIgnored private var pendingRecoveryTask: Task<Void, Never>?
	@ObservationIgnored private var profileRequestGenerations = [String: UInt64]()
	@ObservationIgnored private var profilePrivacyEpoch: UInt64 = 0
	@ObservationIgnored private var codexProjectionRequestGeneration: UInt64 = 0

	init(
		client: any ResetCardClient = DecodexNativeClient(),
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
		pendingRecoveryTask?.cancel()
	}

	var isInitialLoading: Bool {
		isRefreshing && hasLoaded == false
	}

	var canPerformDirectAccountControl: Bool {
		isRefreshingAccountSkeleton == false
			&& (isRefreshing == false || refreshSkeletonIsPublished)
	}

	func blocksNewAttempt(for target: ResetCardUseTarget) -> Bool {
		isPendingRecoveryBlocked
			|| submittingKey != nil
			|| isAwaitingFreshAccountSkeleton(target.accountID)
			|| pendingAttempts.count >= ResetCardPendingAttemptStore.maximumAttempts
			|| pendingAttempts.contains(where: {
				$0.target.accountID == target.accountID
					&& $0.target.descriptor == target.descriptor
			})
	}

	func start() {
		guard startupTask == nil, pendingRecoveryTask == nil else {
			return
		}

		let retryDelays = startupRetryDelays
		startupTask = Task { [weak self] in
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
		pendingRecoveryTask = Task { [weak self] in
			guard var shouldRetry = await self?.recoverPendingAttempts() else {
				return
			}

			for delay in retryDelays {
				guard shouldRetry, Task.isCancelled == false else {
					return
				}

				do {
					try await Task.sleep(for: delay)
				} catch {
					return
				}

				guard let retry = await self?.recoverPendingAttempts() else {
					return
				}
				shouldRetry = retry
			}
		}
	}

	func refresh() async {
		guard isRefreshing == false,
			isRefreshingAccountSkeleton == false,
			isAccountControlInProgress == false
		else {
			return
		}
		startupTask?.cancel()
		startupTask = nil
		pendingRecoveryTask?.cancel()
		pendingRecoveryTask = nil
		clearStaleControlError()
		_ = await refreshReadState()
		_ = await recoverPendingAttempts()
	}

	func setProfileEmailVisibility(_ isVisible: Bool) async {
		let didChange = profileEmailsVisible != isVisible
		profileEmailsVisible = isVisible
		if didChange {
			profilePrivacyEpoch &+= 1
		}
		if isVisible == false {
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
		refreshSkeletonIsPublished = false
		defer {
			isRefreshing = false
			refreshSkeletonIsPublished = false
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
			var projectionReadback: (
				generation: UInt64,
				projection: CodexAuthProjection
			)?
			if let accountControlClient {
				let projectionGeneration = beginCodexProjectionRequest()
				async let snapshotRead = accountControlClient.accountSnapshot(
					authority: retainedAuthority
				)
				async let projectionRead = try? accountControlClient.codexAuthProjection(
					authority: retainedAuthority
				)
				let snapshot = try await snapshotRead
				let projection = await projectionRead ?? .unavailable
				projectionReadback = (projectionGeneration, projection)
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
					?? account.authority
					?? previous?.account.authority
					?? previous?.inventory?.authority
				let boundAccount = Self.account(account, authority: authority)
				let sameRevision = previous?.account.accountRevision == account.accountRevision
				let retriesInventory = sameRevision
					&& (
						previous?.error?.isRetryableReadFailure == true
							|| previous?.inventory?.observationError?.isRetryableReadFailure == true
					)
				let retainedInventory = sameRevision && retriesInventory == false
					? previous?.inventory
					: nil
				let retainedError = sameRevision && retriesInventory == false
					? previous?.error
					: nil
				let retainedProfile = sameRevision
					? previous?.profile
					: nil
				let retriesProfile = sameRevision
					&& (
						previous?.profileError?.isRetryableReadFailure == true
							|| previous?.profileUnavailable?.error.isRetryableReadFailure == true
					)
				let retainedProfileUnavailable = sameRevision && retriesProfile == false
					? previous?.profileUnavailable
					: nil
				let retainedProfileError = sameRevision && retriesProfile == false
					? previous?.profileError
					: nil
				let awaitsNewerSkeleton = accountSkeletonRevisionTargets[
					account.accountID
				].map { account.accountRevision < $0 } ?? false
				return ResetCardAccountState(
					account: boundAccount,
					inventory: retainedInventory,
					error: retainedError,
					isRefreshing: awaitsNewerSkeleton
						|| (retainedInventory == nil && retainedError == nil),
					profile: profileEmailsVisible
						? retainedProfile
						: retainedProfile?.redactingEmail(),
					profileUnavailable: profileEmailsVisible
						? retainedProfileUnavailable
						: retainedProfileUnavailable?.redactingEmail(),
					profileError: retainedProfileError,
					isProfileRefreshing: accountProfileClient != nil
						&& retainedProfile == nil
						&& retainedProfileUnavailable == nil
					&& retainedProfileError == nil
				)
			}
			reconcileAccountSkeletonRevisionTargets()
			if let projectionReadback,
				applyCodexAuthProjection(
					projectionReadback.projection,
					generation: projectionReadback.generation
				) {
				scheduleCodexProjectionRefresh()
			}
			if message?.tone == .error, isPendingRecoveryBlocked == false {
				message = nil
			}
			refreshSkeletonIsPublished = true

			let client = self.client
			let accountProfileClient = self.accountProfileClient
			let includeEmail = profileEmailsVisible
			var profileRequests = [AccountProfileRequest]()
			var expectedAuthority = retainedAuthority
			var reads = [@Sendable () async -> ResetCardAccountRead]()
			for state in accounts {
				let account = state.account
				reads.append {
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
				if let accountProfileClient {
					let profileRequest = AccountProfileRequest(
						generation: beginProfileRequestGeneration(
							accountID: account.accountID
						),
						privacyEpoch: profilePrivacyEpoch,
						includesEmail: includeEmail,
						accountID: account.accountID,
						accountRevision: account.accountRevision
					)
					profileRequests.append(profileRequest)
					reads.append {
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
			await withTaskGroup(of: ResetCardAccountRead.self) { group in
				var nextRead = 0
				let initialCount = min(Self.maximumConcurrentAccountReads, reads.count)
				while nextRead < initialCount {
					group.addTask(operation: reads[nextRead])
					nextRead += 1
				}

				while let read = await group.next() {
					guard Task.isCancelled == false else {
						return
					}
					switch read {
					case .inventoryAvailable(let accountID, let inventory):
						if expectedAuthority == nil || expectedAuthority == inventory.authority {
							expectedAuthority = inventory.authority
							applyInventory(inventory, accountID: accountID)
						} else {
							applyInventoryFailure(
								.invalidResponse,
								accountID: accountID
							)
						}
					case .inventoryFailed(let accountID, let error):
						applyInventoryFailure(error, accountID: accountID)
					case .profileAvailable(let accountID, let request, let profile):
						if isCurrentProfileRequest(request) {
							applyProfile(profile, accountID: accountID, request: request)
						}
					case .profileUnavailable(let accountID, let request, let unavailable):
						if isCurrentProfileRequest(request) {
							applyProfileUnavailable(
								unavailable,
								accountID: accountID,
								request: request
							)
						}
					case .profileFailed(let accountID, let request, let error):
						if isCurrentProfileRequest(request) {
							applyProfileFailure(error, accountID: accountID, request: request)
						}
					}
					if nextRead < reads.count {
						group.addTask(operation: reads[nextRead])
						nextRead += 1
					}
				}
			}
			for request in profileRequests {
				finishProfileRequest(request)
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
		guard isAwaitingFreshAccountSkeleton(attempt.target.accountID) == false else {
			message = ResetCardStoreMessage(
				tone: .information,
				text: "The account state changed. Wait for the account list to refresh."
			)
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
		clearStaleControlError()
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

		clearStaleControlError()
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
					return ResetCardDispatchOutcome.state(
						try await client.status(for: attempt)
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
			return
		}
		_ = await apply(dispatch, to: attempt)
	}

	func dismissMessage() {
		message = nil
	}

	var isAccountControlInProgress: Bool {
		accountControlActivities.isEmpty == false
			|| isEnrollingAccount
			|| isRoutingAccountControl
	}

	func isControllingAccount(_ accountID: String) -> Bool {
		accountControlActivities[accountID] != nil
	}

	func isControllingAccount(
		_ accountID: String,
		activity: AccountControlActivity
	) -> Bool {
		accountControlActivities[accountID] == activity
	}

	func isAwaitingFreshAccountSkeleton(_ accountID: String) -> Bool {
		accountSkeletonRevisionTargets[accountID] != nil
	}

	func enrollFromSharedCodex(enabled: Bool = true) async {
		guard isRefreshing == false,
			isRefreshingAccountSkeleton == false,
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
					enabled: enabled,
					idempotencyKey: idempotencyKey
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
			activity: .lifecycle,
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
			activity: .lifecycle,
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
			isAwaitingFreshAccountSkeleton(accountID) == false,
			let routing,
			routing.order.contains(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		await performAccountControl(
			isRoutingControl: true,
			allowsDuringRefresh: true,
			successMessage: nil,
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
			isRoutingControl: true,
			allowsDuringRefresh: true,
			successMessage: nil,
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
			activity: .loginRefresh,
			allowsDuringRefresh: true,
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

	func useAccountInCodex(_ accountID: String) async {
		guard let account = accountRecord(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		await performAccountControl(
			accountID: accountID,
			activity: .codexProjection,
			allowsDuringRefresh: true,
			successMessage: nil,
			operation: {
				try await accountControlClient.useAccountInCodex(
					authority: account.authority ?? establishedAuthority,
					accountID: accountID,
					expectedRevision: account.accountRevision,
					idempotencyKey: Self.newCanonicalUUID()
				)
			}
		)
	}

	func isCodexProjection(_ accountID: String) -> Bool {
		guard let account = accountRecord(accountID),
			account.accountRevision > 0,
			case .current(let projectedID, let projectedRevision, _) = codexAuthProjection
		else {
			return false
		}
		return projectedID == accountID
			&& projectedRevision == account.accountRevision
	}

	func accountLabel(for accountID: String) -> String {
		accounts.first(where: { $0.account.accountID == accountID })?
			.account.alias
			?? "Unknown account"
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
		case .nativeClientUnavailable, .timedOut, .outputTooLarge,
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
					shouldRetry = true
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

	private func refreshAccountDetails(_ accountID: String) async {
		guard let index = accounts.firstIndex(where: {
			$0.account.accountID == accountID
		}) else {
			return
		}
		let account = accounts[index].account
		let client = self.client
		let includeEmail = profileEmailsVisible
		let profileRequest = accountProfileClient.map { _ in
			AccountProfileRequest(
				generation: beginProfileRequestGeneration(accountID: accountID),
				privacyEpoch: profilePrivacyEpoch,
				includesEmail: includeEmail,
				accountID: accountID,
				accountRevision: account.accountRevision
			)
		}
		defer {
			if let profileRequest {
				finishProfileRequest(profileRequest)
			}
		}

		await withTaskGroup(of: ResetCardAccountRead.self) { group in
			group.addTask {
				do {
					return .inventoryAvailable(
						accountID: accountID,
						try await client.inventory(for: account)
					)
				} catch {
					return .inventoryFailed(
						accountID: accountID,
						Self.clientError(error)
					)
				}
			}
			if let accountProfileClient, let profileRequest {
				group.addTask {
					do {
						switch try await accountProfileClient.profile(
							for: account,
							includeEmail: includeEmail
						) {
						case .available(let profile):
							return .profileAvailable(
								accountID: accountID,
								request: profileRequest,
								profile
							)
						case .unavailable(let unavailable):
							return .profileUnavailable(
								accountID: accountID,
								request: profileRequest,
								unavailable
							)
						}
					} catch {
						return .profileFailed(
							accountID: accountID,
							request: profileRequest,
							Self.clientError(error)
						)
					}
				}
			}

			for await read in group {
				switch read {
				case .inventoryAvailable(let id, let inventory):
					applyInventory(inventory, accountID: id)
				case .inventoryFailed(let id, let error):
					applyInventoryFailure(error, accountID: id)
				case .profileAvailable(let id, let request, let profile):
					applyProfile(profile, accountID: id, request: request)
				case .profileUnavailable(let id, let request, let unavailable):
					applyProfileUnavailable(
						unavailable,
						accountID: id,
						request: request
					)
				case .profileFailed(let id, let request, let error):
					applyProfileFailure(error, accountID: id, request: request)
				}
			}
		}
	}

	private func refreshAccountSkeleton() async {
		guard let accountControlClient,
			isRefreshingAccountSkeleton == false,
			isAccountControlInProgress == false
		else {
			return
		}
		isRefreshingAccountSkeleton = true
		defer {
			isRefreshingAccountSkeleton = false
		}
		do {
			let projectionGeneration = beginCodexProjectionRequest()
			async let snapshotRead = accountControlClient.accountSnapshot(
				authority: establishedAuthority
			)
			async let projectionRead = try? accountControlClient.codexAuthProjection(
				authority: establishedAuthority
			)
			let snapshot = try await snapshotRead
			let previousByID = Dictionary(
				uniqueKeysWithValues: accounts.map {
					($0.account.accountID, $0)
				}
			)
			routing = snapshot.routing
			var accountsNeedingDetails = [String]()
			accounts = snapshot.accounts.map { account in
				let previous = previousByID[account.accountID]
				let authority = snapshot.authority
					?? account.authority
					?? previous?.account.authority
					?? previous?.inventory?.authority
				let bound = Self.account(account, authority: authority)
				let sameRevision = previous?.account.accountRevision
					== bound.accountRevision
				let awaitsNewerSkeleton = accountSkeletonRevisionTargets[
					bound.accountID
				].map { bound.accountRevision < $0 } ?? false
				if sameRevision == false {
					accountsNeedingDetails.append(bound.accountID)
				}
				return ResetCardAccountState(
					account: bound,
					inventory: sameRevision ? previous?.inventory : nil,
					error: sameRevision ? previous?.error : nil,
					isRefreshing: sameRevision == false || awaitsNewerSkeleton,
					profile: sameRevision ? previous?.profile : nil,
					profileUnavailable: sameRevision
						? previous?.profileUnavailable
						: nil,
					profileError: sameRevision ? previous?.profileError : nil,
					isProfileRefreshing: sameRevision == false
					&& accountProfileClient != nil
				)
			}
			reconcileAccountSkeletonRevisionTargets()
			for accountID in accountsNeedingDetails {
				scheduleAccountControlFollowUp(.account(accountID))
			}
			if applyCodexAuthProjection(
				await projectionRead ?? .unavailable,
				generation: projectionGeneration
			) {
				scheduleCodexProjectionRefresh()
			}
		} catch {
			message = ResetCardStoreMessage(
				tone: .error,
				text: Self.clientError(error).localizedDescription
			)
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
		guard inventory.accountRevision >= existing.accountRevision else {
			return
		}
		guard inventory.accountRevision == existing.accountRevision else {
			rejectAdvancedInventory(
				accountID: accountID,
				inventoryRevision: inventory.accountRevision,
				index: index
			)
			return
		}
		let account = ResetCardAccountRecord(
			authority: inventory.authority,
			accountID: existing.accountID,
			alias: existing.alias,
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
		let revisionChanged = retainsProfileState == false
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
		if revisionChanged {
			invalidateCodexProjectionAfterRevisionChange(
				accountID: accountID,
				accountRevision: inventory.accountRevision
			)
		}
	}

	private func rejectAdvancedInventory(
		accountID: String,
		inventoryRevision: UInt64,
		index: Int
	) {
		let existing = accounts[index]
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: existing.error,
			isRefreshing: true,
			profile: existing.profile,
			profileUnavailable: existing.profileUnavailable,
			profileError: existing.profileError,
			isProfileRefreshing: existing.isProfileRefreshing
		)

		if let target = accountSkeletonRevisionTargets[accountID] {
			accountSkeletonRevisionTargets[accountID] = max(target, inventoryRevision)
			return
		}
		accountSkeletonRevisionTargets[accountID] = inventoryRevision
		scheduleFreshAccountSkeletonRead()
	}

	private func scheduleFreshAccountSkeletonRead() {
		guard accountSkeletonRevisionTargets.isEmpty == false,
			isAccountControlInProgress == false
		else {
			return
		}
		Task { [weak self] in
			await self?.refreshAccountSkeleton()
		}
	}

	private func reconcileAccountSkeletonRevisionTargets() {
		let revisionsByID = Dictionary(
			uniqueKeysWithValues: accounts.map {
				($0.account.accountID, $0.account.accountRevision)
			}
		)
		accountSkeletonRevisionTargets = accountSkeletonRevisionTargets.filter {
			accountID,
			targetRevision in
			guard let revision = revisionsByID[accountID] else {
				return false
			}
			return revision < targetRevision
		}
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
		var requests = [AccountProfileRequest]()
		defer {
			for request in requests {
				finishProfileRequest(request)
			}
		}
		await withTaskGroup(of: ResetCardAccountRead.self) { group in
			for state in accounts {
				let account = state.account
				let request = AccountProfileRequest(
					generation: beginProfileRequestGeneration(
						accountID: account.accountID
					),
					privacyEpoch: profilePrivacyEpoch,
					includesEmail: includeEmail,
					accountID: account.accountID,
					accountRevision: account.accountRevision
				)
				requests.append(request)
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

	private func beginProfileRequestGeneration(accountID: String) -> UInt64 {
		let generation = (profileRequestGenerations[accountID] ?? 0) &+ 1
		profileRequestGenerations[accountID] = generation
		return generation
	}

	private func isCurrentProfileRequest(_ request: AccountProfileRequest) -> Bool {
		guard request.generation == profileRequestGenerations[request.accountID],
			request.privacyEpoch == profilePrivacyEpoch,
			request.includesEmail == profileEmailsVisible,
			let state = accounts.first(where: {
				$0.account.accountID == request.accountID
			})
		else {
			return false
		}
		return state.account.accountRevision == request.accountRevision
	}

	private func finishProfileRequest(_ request: AccountProfileRequest) {
		guard request.generation == profileRequestGenerations[request.accountID],
			request.privacyEpoch == profilePrivacyEpoch,
			let index = accounts.firstIndex(where: {
				$0.account.accountID == request.accountID
			}),
			accounts[index].isProfileRefreshing
		else {
			return
		}
		let state = accounts[index]
		accounts[index] = ResetCardAccountState(
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

	private static func account(
		_ account: ResetCardAccountRecord,
		authority: ResetCardAuthority?
	) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: authority,
			accountID: account.accountID,
			alias: account.alias,
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
		activity: AccountControlActivity? = nil,
		isEnrollment: Bool = false,
		isRoutingControl: Bool = false,
		allowsDuringRefresh: Bool = false,
		successMessage: String?,
		operation: () async throws -> AccountControlResult
	) async {
		guard isRefreshingAccountSkeleton == false,
			isRefreshing == false
				|| (allowsDuringRefresh && refreshSkeletonIsPublished)
		else {
			return
		}
		if let accountID {
			guard activity != nil,
				accountSkeletonRevisionTargets[accountID] == nil,
				accountControlActivities[accountID] == nil,
				isEnrollingAccount == false,
				isRoutingAccountControl == false
			else {
				return
			}
		} else if isRoutingControl {
			guard isRoutingAccountControl == false,
				accountControlActivities.isEmpty
			else {
				return
			}
		} else if isEnrollment == false {
			return
		}
		if isEnrollment {
			guard isEnrollingAccount,
				accountControlActivities.isEmpty,
				isRoutingAccountControl == false
			else {
				return
			}
		}

		if let accountID, let activity {
			accountControlActivities[accountID] = activity
		} else if isRoutingControl {
			isRoutingAccountControl = true
		}
		clearStaleControlError()
		defer {
			if let accountID {
				accountControlActivities.removeValue(forKey: accountID)
			} else if isRoutingControl {
				isRoutingAccountControl = false
			}
			scheduleFreshAccountSkeletonRead()
		}

		do {
			let result = try await operation()
			let followUp = applyAccountControlResult(result)
			if let successMessage {
				message = ResetCardStoreMessage(tone: .success, text: successMessage)
			}
			scheduleAccountControlFollowUp(followUp)
		} catch let error as AccountControlError {
			message = ResetCardStoreMessage(
				tone: .error,
				text: error.localizedDescription
			)
		} catch let error as ResetCardClientError {
			message = ResetCardStoreMessage(
				tone: .error,
				text: error.localizedDescription
			)
		} catch {
			message = ResetCardStoreMessage(
				tone: .error,
				text: AccountControlError.invalidResponse.localizedDescription
			)
		}
	}

	private enum AccountControlFollowUp {
		case none
		case account(String)
		case skeleton
	}

	private func applyAccountControlResult(
		_ result: AccountControlResult
	) -> AccountControlFollowUp {
		switch result {
		case .codexAuthProjected(let accountID, let accountRevision, let projectionDigest):
			codexProjectionRequestGeneration &+= 1
			codexAuthProjection = .current(
				accountID: accountID,
				accountRevision: accountRevision,
				projectionDigest: projectionDigest
			)
			return .none
		case .routingChanged(let routing):
			self.routing = routing
			return .none
		case .accountLoggedOut(let accountID, _):
			if case .current(let projectedID, _, _) = codexAuthProjection,
				projectedID == accountID
			{
				codexProjectionRequestGeneration &+= 1
				codexAuthProjection = nil
			}
			accounts.removeAll { $0.account.accountID == accountID }
			accountSkeletonRevisionTargets.removeValue(forKey: accountID)
			return .skeleton
		case .accountChanged(let account):
			guard let index = accounts.firstIndex(where: {
				$0.account.accountID == account.accountID
			}) else {
				return .skeleton
			}
			let existing = accounts[index]
			let authority = account.authority
				?? existing.account.authority
				?? existing.inventory?.authority
			let bound = Self.account(account, authority: authority)
			let sameRevision = existing.account.accountRevision == bound.accountRevision
			accounts[index] = ResetCardAccountState(
				account: bound,
				inventory: sameRevision ? existing.inventory : nil,
				error: nil,
				isRefreshing: sameRevision == false,
				profile: sameRevision ? existing.profile : nil,
				profileUnavailable: sameRevision ? existing.profileUnavailable : nil,
				profileError: nil,
				isProfileRefreshing: sameRevision == false && accountProfileClient != nil
			)
			reconcileAccountSkeletonRevisionTargets()
			if sameRevision == false {
				invalidateCodexProjectionAfterRevisionChange(
					accountID: account.accountID,
					accountRevision: bound.accountRevision
				)
			}
			return sameRevision ? .none : .account(account.accountID)
		}
	}

	private func scheduleAccountControlFollowUp(
		_ followUp: AccountControlFollowUp
	) {
		switch followUp {
		case .none:
			return
		case .account(let accountID):
			Task { [weak self] in
				await self?.refreshAccountDetails(accountID)
			}
		case .skeleton:
			Task { [weak self] in
				await self?.refreshAccountSkeleton()
			}
		}
	}

	private func presentAccountControlUnavailable() {
		message = ResetCardStoreMessage(
			tone: .error,
			text: "Account controls are unavailable while another account request is active."
		)
	}

	private func clearStaleControlError() {
		guard isPendingRecoveryBlocked == false, message?.tone == .error else {
			return
		}
		message = nil
	}

	private func beginCodexProjectionRequest() -> UInt64 {
		codexProjectionRequestGeneration &+= 1
		return codexProjectionRequestGeneration
	}

	@discardableResult
	private func applyCodexAuthProjection(
		_ projection: CodexAuthProjection,
		generation: UInt64
	) -> Bool {
		guard generation == codexProjectionRequestGeneration else {
			return false
		}
		if case .current(let accountID, let accountRevision, _) = projection {
			guard accountRevision > 0,
				let account = accountRecord(accountID),
				account.accountRevision == accountRevision
			else {
				codexAuthProjection = nil
				return true
			}
		}
		codexAuthProjection = projection
		return false
	}

	private func invalidateCodexProjectionAfterRevisionChange(
		accountID: String,
		accountRevision: UInt64
	) {
		codexProjectionRequestGeneration &+= 1
		if case .current(let projectedID, let projectedRevision, _) = codexAuthProjection,
			projectedID == accountID,
			projectedRevision != accountRevision
		{
			codexAuthProjection = nil
		}
		scheduleCodexProjectionRefresh()
	}

	private func scheduleCodexProjectionRefresh() {
		Task { [weak self] in
			await self?.refreshCodexAuthProjection()
		}
	}

	private func refreshCodexAuthProjection() async {
		guard let accountControlClient else {
			return
		}
		let generation = beginCodexProjectionRequest()
		let projection = (try? await accountControlClient.codexAuthProjection(
			authority: establishedAuthority
		)) ?? .unavailable
		_ = applyCodexAuthProjection(projection, generation: generation)
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
		case .nativeClientUnavailable, .timedOut, .commandFailed:
			return true
		case .service(let error):
			return error.isRetryableReadFailure
		case .outputTooLarge, .commandRejected,
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
