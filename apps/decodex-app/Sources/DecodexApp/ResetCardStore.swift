import Foundation
import Observation

enum ResetCardInventoryFailure: Equatable {
	case updating(detail: String)
	case connecting(detail: String)
	case unavailable(detail: String)
}

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
		Self.preferredQuota(
			inventory?.fiveHourQuota,
			account.fiveHourQuota
		)
	}

	var sevenDayQuota: ResetCardQuotaWindow {
		Self.preferredQuota(
			inventory?.sevenDayQuota,
			account.sevenDayQuota
		)
	}

	var inventoryIsCurrent: Bool {
		guard let inventory else {
			return false
		}
		return inventory.accountID == account.accountID
			&& inventory.accountRevision == account.accountRevision
	}

	var inventoryFailure: ResetCardInventoryFailure? {
		if let error {
			if error.isConnectionFailure {
				return .connecting(detail: error.localizedDescription)
			}
			return error.isRetryableReadFailure
				? .updating(detail: error.localizedDescription)
				: .unavailable(detail: error.localizedDescription)
		}
		if let error = inventory?.observationError {
			return error.isRetryableReadFailure
				? .updating(detail: error.presentation)
				: .unavailable(detail: error.presentation)
		}
		return nil
	}

	private static func preferredQuota(
		_ inventory: ResetCardQuotaWindow?,
		_ account: ResetCardQuotaWindow
	) -> ResetCardQuotaWindow {
		guard let inventory else {
			return account
		}
		guard case .current = account.state else {
			return inventory
		}
		guard case .current = inventory.state else {
			return account
		}
		return (account.observedAtUnixMicros ?? 0)
			> (inventory.observedAtUnixMicros ?? 0)
			? account
			: inventory
	}

	var targets: [ResetCardUseTarget] {
		guard let inventory,
			inventoryIsCurrent,
			error == nil,
			inventory.observationError == nil,
			inventory.detailsComplete
		else {
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
		if account.observedState == .authFailed {
			return true
		}
		return profileUnavailable?.error == .unauthorized
			|| profile?.refreshError == .unauthorized
	}
}

private struct AccountProfileRequest: Equatable, Sendable {
	let generation: UInt64
	let includesEmail: Bool
	let accountID: String
	let accountRevision: UInt64
}

private struct CachedAccountEmail: Equatable, Sendable {
	let accountRevision: UInt64
	let email: String?
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

enum ResetCardPendingStatus: Equatable {
	case checking(detail: String?)
	case retrying(detail: String)

	var text: String {
		switch self {
		case .checking:
			return "Checking reset result…"
		case .retrying:
			return "Check delayed; retrying…"
		}
	}

	var accessibilityText: String {
		switch self {
		case .checking:
			return "Checking reset result"
		case .retrying:
			return "Reset result check delayed; retrying"
		}
	}

	var detail: String? {
		switch self {
		case .checking(let detail):
			return detail
		case .retrying(let detail):
			return detail
		}
	}
}

enum AccountControlActivity: Equatable {
	case lifecycle
	case loginRefresh
	case route
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

private enum ResetCardInventoryRefreshResult: Equatable {
	case current
	case awaitingSkeleton
	case retryNeeded
	case failed
	case missing
}

@MainActor
@Observable
final class ResetCardStore {
	// These reads return daemon-owned observations and never start provider work.
	// Fetch every independent account projection concurrently so one row cannot
	// delay another row's first presentation.
	private static let defaultObservationSignalReconnectDelays: [Duration] = [
		.milliseconds(250),
		.milliseconds(500),
		.seconds(1),
		.seconds(2),
		.seconds(4),
		.seconds(8),
	]
	private static let defaultPostUseRetryDelays: [Duration] = [
		.seconds(1),
		.seconds(3),
		.seconds(7),
	]

	private static let defaultStartupRetryDelays: [Duration] = [
		.seconds(1),
		.seconds(2),
		.seconds(4),
		.seconds(8),
		.seconds(15),
		.seconds(30),
	]
	private static let defaultAccountObservationRetryDelays: [Duration] = [
		.milliseconds(250),
		.milliseconds(500),
		.seconds(1),
		.seconds(2),
		.seconds(4),
		.seconds(8),
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
	private(set) var pendingStatuses: [String: ResetCardPendingStatus]
	private(set) var isPendingRecoveryBlocked: Bool
	private(set) var profileEmailsVisible = false
	private(set) var accountReauthentication: AccountReauthenticationPresentation?
	var message: ResetCardStoreMessage?

	@ObservationIgnored private let client: any ResetCardClient
	@ObservationIgnored private let inventoryReads: ResetCardInventoryReadCoordinator
	@ObservationIgnored private let accountControlClient: (any AccountControlClient)?
	@ObservationIgnored private let accountProfileClient: (any AccountProfileClient)?
	@ObservationIgnored private let accountObservationClient: (any AccountObservationClient)?
	@ObservationIgnored private let pendingStore: ResetCardPendingAttemptStore
	@ObservationIgnored private let startupRetryDelays: [Duration]
	@ObservationIgnored private let postUseRetryDelays: [Duration]
	@ObservationIgnored private let observationSignalReconnectDelays: [Duration]
	@ObservationIgnored private let accountReauthenticationPollInterval: Duration
	@ObservationIgnored private let accountObservationRetryDelays: [Duration]
	@ObservationIgnored private let resolveCodexExecutable: @MainActor @Sendable () throws -> String
	@ObservationIgnored private var startupTask: Task<Void, Never>?
	@ObservationIgnored private var accountObservationTask: Task<Void, Never>?
	@ObservationIgnored private var refreshCycleTask: Task<Void, Never>?
	@ObservationIgnored private var pendingRecoveryTask: Task<Void, Never>?
	@ObservationIgnored private var accountReauthenticationTask: Task<Void, Never>?
	@ObservationIgnored private var postUseReconciliationTasks = [
		String: Task<Void, Never>
	]()
	@ObservationIgnored private var postUseReconciliationAccountIDs = Set<String>()
	@ObservationIgnored private(set) var isPreparingForTermination = false
	@ObservationIgnored private var profileRequestGenerations = [String: UInt64]()
	@ObservationIgnored private var profileEmailCache = [String: CachedAccountEmail]()
	@ObservationIgnored private var requestedProfileEmailVisibility = false
	@ObservationIgnored private var profilePrivacyEpoch: UInt64 = 0
	@ObservationIgnored private var codexProjectionRequestGeneration: UInt64 = 0
	@ObservationIgnored private var accountSkeletonRefreshGeneration: UInt64 = 0
	@ObservationIgnored private var advancedInventoriesAwaitingSkeleton = [
		String: ResetCardInventory
	]()

	init(
		client: any ResetCardClient = DecodexNativeClient(),
		pendingStore: ResetCardPendingAttemptStore = ResetCardPendingAttemptStore(),
		startupRetryDelays: [Duration] = ResetCardStore.defaultStartupRetryDelays,
		postUseRetryDelays: [Duration] = ResetCardStore.defaultPostUseRetryDelays,
		observationSignalReconnectDelays: [Duration] = ResetCardStore
			.defaultObservationSignalReconnectDelays,
		accountReauthenticationPollInterval: Duration = .seconds(1),
		accountObservationRetryDelays: [Duration] = ResetCardStore
			.defaultAccountObservationRetryDelays,
		resolveCodexExecutable: @escaping @MainActor @Sendable () throws -> String = {
			try CodexExecutableResolver.resolve()
		}
	) {
		self.client = client
		inventoryReads = ResetCardInventoryReadCoordinator(client: client)
		accountControlClient = client as? any AccountControlClient
		accountProfileClient = client as? any AccountProfileClient
		accountObservationClient = client as? any AccountObservationClient
		self.pendingStore = pendingStore
		self.startupRetryDelays = startupRetryDelays
		self.postUseRetryDelays = postUseRetryDelays
		self.observationSignalReconnectDelays = observationSignalReconnectDelays
		self.accountReauthenticationPollInterval = accountReauthenticationPollInterval
		self.accountObservationRetryDelays = accountObservationRetryDelays
		self.resolveCodexExecutable = resolveCodexExecutable
		let pendingLoad = pendingStore.load()
		pendingAttempts = pendingLoad.attempts
		pendingStatuses = Dictionary(
			uniqueKeysWithValues: pendingLoad.attempts.map {
				($0.idempotencyKey, ResetCardPendingStatus.checking(detail: nil))
			}
		)
		isPendingRecoveryBlocked = pendingLoad.isRecoveryBlocked
		if isPendingRecoveryBlocked {
			message = Self.pendingRecoveryBlockedMessage
		}
	}

	deinit {
		startupTask?.cancel()
		accountObservationTask?.cancel()
		refreshCycleTask?.cancel()
		pendingRecoveryTask?.cancel()
		accountReauthenticationTask?.cancel()
		for task in postUseReconciliationTasks.values {
			task.cancel()
		}
	}

	var isInitialLoading: Bool {
		isRefreshing && hasLoaded == false
	}

	var canPerformDirectAccountControl: Bool {
		isRefreshingAccountSkeleton == false
			&& (isRefreshing == false || refreshSkeletonIsPublished)
	}

	var canBeginEnrollment: Bool {
		accountControlClient != nil
			&& canPerformDirectAccountControl
			&& isAccountControlInProgress == false
	}

	var canReorderAccounts: Bool {
		guard let routing else {
			return false
		}
		let accountIDs = accounts.map { $0.account.accountID }
		return accountControlClient != nil
			&& accountIDs.count > 1
			&& accountIDs.count == routing.order.count
			&& Set(accountIDs) == Set(routing.order)
			&& canPerformDirectAccountControl
			&& isAccountControlInProgress == false
			&& submittingKey == nil
	}

	func blocksNewAttempt(for target: ResetCardUseTarget) -> Bool {
		isPendingRecoveryBlocked
			|| submittingKey != nil
			|| postUseReconciliationAccountIDs.contains(target.accountID)
			|| isAwaitingFreshAccountSkeleton(target.accountID)
			|| pendingAttempts.count >= ResetCardPendingAttemptStore.maximumAttempts
			|| pendingAttempts.contains(where: {
				$0.target.accountID == target.accountID
					&& $0.target.descriptor == target.descriptor
			})
	}

	func pendingStatus(for attempt: ResetCardUseAttempt) -> ResetCardPendingStatus {
		pendingStatuses[attempt.idempotencyKey] ?? .checking(detail: nil)
	}

	func start() {
		guard startupTask == nil,
			accountObservationTask == nil,
			pendingRecoveryTask == nil
		else {
			return
		}

		startAccountObservationSignals()
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

	func prepareForApplicationTermination() async {
		guard isPreparingForTermination == false else {
			return
		}
		isPreparingForTermination = true

		let inFlightStartupTask = startupTask
		let inFlightRefreshCycleTask = refreshCycleTask
		let inFlightPendingRecoveryTask = pendingRecoveryTask
		let inFlightAccountReauthenticationTask = accountReauthenticationTask
		let inFlightPostUseReconciliationTasks = Array(
			postUseReconciliationTasks.values
		)

		startupTask?.cancel()
		startupTask = nil
		accountObservationTask?.cancel()
		accountObservationTask = nil
		refreshCycleTask?.cancel()
		refreshCycleTask = nil
		pendingRecoveryTask?.cancel()
		pendingRecoveryTask = nil
		accountReauthenticationTask?.cancel()
		accountReauthenticationTask = nil
		for task in inFlightPostUseReconciliationTasks {
			task.cancel()
		}
		postUseReconciliationTasks.removeAll()

		await inFlightStartupTask?.value
		// The native wait is a bounded synchronous FFI request. Client destruction
		// safely releases its in-flight Arc, so termination cancels this owner but
		// does not wait for a daemon heartbeat before shutting down the shared client.
		await inFlightRefreshCycleTask?.value
		await inFlightPendingRecoveryTask?.value
		await inFlightAccountReauthenticationTask?.value
		for task in inFlightPostUseReconciliationTasks {
			await task.value
		}
		await inventoryReads.cancelAll()
	}

	private func startAccountObservationSignals() {
		guard let accountObservationClient else {
			return
		}
		let reconnectDelays = observationSignalReconnectDelays
		accountObservationTask = Task { [weak self] in
			var generation: UInt64 = 0
			var failureCount = 0
			while Task.isCancelled == false {
				do {
					let signal = try await accountObservationClient.waitForAccountObservation(
						afterGeneration: generation
					)
					guard Task.isCancelled == false, let self else {
						return
					}
					generation = signal.generation
					failureCount = 0
					self.requestRefresh()
				} catch {
					guard Task.isCancelled == false, reconnectDelays.isEmpty == false else {
						return
					}
					let index = min(failureCount, reconnectDelays.count - 1)
					failureCount = min(failureCount + 1, reconnectDelays.count - 1)
					do {
						try await Task.sleep(for: reconnectDelays[index])
					} catch {
						return
					}
				}
			}
		}
	}

	func requestRefresh() {
		guard isPreparingForTermination == false,
			refreshCycleTask == nil
		else {
			return
		}

		refreshCycleTask = Task { [weak self] in
			guard let self else {
				return
			}
			await self.performRefreshCycle()
			self.refreshCycleTask = nil
		}
	}

	func refresh() async {
		requestRefresh()
		await refreshCycleTask?.value
	}

	private func performRefreshCycle() async {
		guard isRefreshing == false,
			isRefreshingAccountSkeleton == false,
			isAccountControlInProgress == false
		else {
			return
		}
		let inFlightStartupTask = startupTask
		let inFlightPendingRecoveryTask = pendingRecoveryTask
		startupTask?.cancel()
		startupTask = nil
		pendingRecoveryTask?.cancel()
		pendingRecoveryTask = nil
		await inFlightStartupTask?.value
		await inFlightPendingRecoveryTask?.value

		guard Task.isCancelled == false else {
			return
		}
		clearStaleControlError()
		_ = await refreshReadState()
		guard Task.isCancelled == false else {
			return
		}
		_ = await recoverPendingAttempts()
	}

	func setProfileEmailVisibility(_ isVisible: Bool) async {
		let didChange = requestedProfileEmailVisibility != isVisible
		requestedProfileEmailVisibility = isVisible
		if didChange {
			profilePrivacyEpoch &+= 1
		}
		if isVisible == false {
			profileEmailsVisible = false
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

		guard didChange else {
			return
		}
		let visibilityEpoch = profilePrivacyEpoch
		if publishProfileEmailsIfReady(expectedEpoch: visibilityEpoch) {
			return
		}
		guard accountProfileClient != nil else {
			return
		}
		await refreshProfiles()
		_ = publishProfileEmailsIfReady(expectedEpoch: visibilityEpoch)
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
				let retainedInventory = previous?.inventory
				let retainedError = sameRevision
					? previous?.error
					: nil
				let inventoryIsStale = retainedInventory.map {
					$0.accountRevision != boundAccount.accountRevision
				} ?? false
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
						|| inventoryIsStale
						|| retriesInventory
						|| postUseReconciliationAccountIDs.contains(
							account.accountID
						)
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
			prunePostUseReconciliationsForCurrentAccounts()
			pruneProfileEmailCache()
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

			let inventoryReads = self.inventoryReads
			let accountProfileClient = self.accountProfileClient
			let includeEmail = true
			var profileRequests = [AccountProfileRequest]()
			let visibilityEpoch = profilePrivacyEpoch
			var expectedAuthority = retainedAuthority
			var reads = [@Sendable () async -> ResetCardAccountRead]()
			for state in accounts {
				let account = state.account
				reads.append {
					do {
						return .inventoryAvailable(
							accountID: account.accountID,
							try await inventoryReads.inventory(for: account)
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
				for read in reads {
					group.addTask(operation: read)
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
				}
			}
			for request in profileRequests {
				finishProfileRequest(request)
			}
			_ = publishProfileEmailsIfReady(expectedEpoch: visibilityEpoch)
			shouldRetry = accounts.contains { state in
				state.error?.isRetryableReadFailure == true
					|| state.inventory?.observationError?.isRetryableReadFailure == true
					|| state.profileError?.isRetryableReadFailure == true
					|| state.profileUnavailable?.error.isRetryableReadFailure == true
					|| state.profile?.refreshError?.isRetryableReadFailure == true
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
		if let pendingAttempt = pendingAttempts.first(where: {
			$0.target.accountID == attempt.target.accountID
				&& $0.target.descriptor == attempt.target.descriptor
		}) {
			clearStaleControlError()
			setPendingStatus(
				.checking(
					detail: "This saved Reset Card request is already being checked automatically."
				),
				for: pendingAttempt
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

	func checkPendingStatus(_ attempt: ResetCardUseAttempt) async {
		guard submittingKey == nil,
			pendingAttempts.contains(attempt)
		else {
			return
		}

		clearStaleControlError()
		setPendingStatus(.checking(detail: nil), for: attempt)
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
			if isPendingRecoveryBlocked {
				message = Self.pendingRecoveryBlockedMessage
			} else {
				setPendingStatus(
					.retrying(detail: Self.pendingDispatchUnavailableDetail),
					for: attempt
				)
			}
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
		guard canBeginEnrollment,
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
			allowsDuringRefresh: true,
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
			allowsDuringRefresh: true,
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
			allowsDuringRefresh: true,
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

	func routeAccount(_ accountID: String) async {
		guard let account = accountRecord(accountID),
			isAwaitingFreshAccountSkeleton(accountID) == false,
			let routing,
			routing.order.contains(accountID),
			let accountControlClient
		else {
			presentAccountControlUnavailable()
			return
		}
		let needsCodexProjection = isCodexProjection(accountID) == false
		let needsFixedRouting: Bool
		if case .fixed(let fixedAccountID) = routing.mode {
			needsFixedRouting = fixedAccountID != accountID
		} else {
			needsFixedRouting = true
		}
		guard needsCodexProjection || needsFixedRouting else {
			return
		}

		await performAccountControl(
			accountID: accountID,
			activity: .route,
			isRoutingControl: true,
			allowsDuringRefresh: true,
			successMessage: nil,
			operation: {
				if needsCodexProjection {
					let projectionResult = try await accountControlClient.useAccountInCodex(
						authority: account.authority ?? establishedAuthority,
						accountID: accountID,
						expectedRevision: account.accountRevision,
						idempotencyKey: Self.newCanonicalUUID()
					)
					if needsFixedRouting == false {
						return projectionResult
					}
					_ = applyAccountControlResult(projectionResult)
				}

				return try await accountControlClient.setFixedSelection(
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

	func moveAccount(_ accountID: String, onto targetAccountID: String) async {
		guard canReorderAccounts,
			accountID != targetAccountID,
			let routing,
			let accountControlClient,
			let sourceIndex = routing.order.firstIndex(of: accountID),
			let targetIndex = routing.order.firstIndex(of: targetAccountID)
		else {
			return
		}

		var order = routing.order
		order.remove(at: sourceIndex)
		guard let retainedTargetIndex = order.firstIndex(of: targetAccountID) else {
			return
		}
		let insertionIndex = sourceIndex < targetIndex
			? retainedTargetIndex + 1
			: retainedTargetIndex
		order.insert(accountID, at: insertionIndex)
		guard reorderAccountStates(to: order) else {
			return
		}
		await persistAccountOrder(
			order,
			replacing: routing,
			using: accountControlClient
		)
	}

	func moveAccounts(
		_ accountIDs: [String],
		before targetAccountID: String?
	) async {
		guard canReorderAccounts,
			let routing,
			let accountControlClient,
			accountIDs.isEmpty == false
		else {
			return
		}
		let movingIDs = Set(accountIDs)
		guard movingIDs.count == accountIDs.count,
			movingIDs.isSubset(of: Set(routing.order)),
			targetAccountID.map({ movingIDs.contains($0) == false }) ?? true
		else {
			return
		}

		let movingOrder = routing.order.filter(movingIDs.contains)
		var order = routing.order.filter { movingIDs.contains($0) == false }
		let insertionIndex: Int
		if let targetAccountID {
			guard let targetIndex = order.firstIndex(of: targetAccountID) else {
				return
			}
			insertionIndex = targetIndex
		} else {
			insertionIndex = order.endIndex
		}
		order.insert(contentsOf: movingOrder, at: insertionIndex)
		guard order != routing.order,
			reorderAccountStates(to: order)
		else {
			return
		}
		await persistAccountOrder(
			order,
			replacing: routing,
			using: accountControlClient
		)
	}

	private func persistAccountOrder(
		_ order: [String],
		replacing routing: AccountRoutingControl,
		using accountControlClient: any AccountControlClient
	) async {
		await performAccountControl(
			isRoutingControl: true,
			allowsDuringRefresh: true,
			successMessage: nil,
			operation: {
				try await accountControlClient.setAccountOrder(
					authority: establishedAuthority,
					order: order,
					expectedRoutingRevision: routing.revision,
					idempotencyKey: Self.newCanonicalUUID()
				)
			}
		)
		_ = reorderAccountStates(to: self.routing?.order ?? routing.order)
	}

	func beginAccountReauthentication(for accountID: String) {
		guard accountReauthentication == nil,
			accountReauthenticationTask == nil,
			let state = accounts.first(where: {
				$0.account.accountID == accountID
			}),
			state.requiresLoginRefresh,
			state.account.credentialBinding != nil,
			let accountControlClient,
			canPerformDirectAccountControl,
			isAwaitingFreshAccountSkeleton(accountID) == false,
			accountControlActivities[accountID] == nil,
			isEnrollingAccount == false,
			isRoutingAccountControl == false,
			submittingKey == nil
		else {
			presentAccountControlUnavailable()
			return
		}

		let sessionID = Self.newCanonicalUUID()
		let operationID = Self.newCanonicalUUID()
		let idempotencyKey = Self.newCanonicalUUID()
		let account = state.account
		let authority = account.authority ?? establishedAuthority
		accountControlActivities[accountID] = .loginRefresh
		clearStaleControlError()
		accountReauthentication = AccountReauthenticationPresentation(
			accountID: accountID,
			accountLabel: AccountIdentityPresentation(
				alias: account.alias,
				email: state.profile?.email
					?? state.profileUnavailable?.claims.email,
				revealsEmail: profileEmailsVisible
			).text,
			sessionID: sessionID,
			authority: authority,
			phase: .resolvingCodex
		)
		accountReauthenticationTask = Task { [weak self] in
			await self?.runAccountReauthentication(
				client: accountControlClient,
				authority: authority,
				account: account,
				sessionID: sessionID,
				operationID: operationID,
				idempotencyKey: idempotencyKey
			)
		}
	}

	func cancelAccountReauthentication() async {
		guard let presentation = accountReauthentication,
			let accountControlClient,
			presentation.canRequestCancellation
		else {
			return
		}
		if presentation.canCloseWithoutCancellation {
			closeAccountReauthentication()
			return
		}

		do {
			let status = try await accountControlClient.cancelAccountReauthentication(
				authority: presentation.authority,
				sessionID: presentation.sessionID
			)
			guard accountReauthentication?.sessionID == presentation.sessionID else {
				return
			}
			switch status.state {
			case .cancelled:
				accountReauthenticationTask?.cancel()
				accountReauthenticationTask = nil
				finishAccountReauthentication(
					accountID: presentation.accountID,
					sessionID: presentation.sessionID
				)
			case .completed:
				accountReauthenticationTask?.cancel()
				accountReauthenticationTask = nil
				await completeAccountReauthentication(
					accountID: presentation.accountID,
					sessionID: presentation.sessionID
				)
			case .failed:
				accountReauthenticationTask?.cancel()
				accountReauthenticationTask = nil
				await failAccountReauthentication(
					accountID: presentation.accountID,
					sessionID: presentation.sessionID,
					message: status.failure?.presentation
						?? AccountControlError.invalidResponse.localizedDescription,
					failure: status.failure
				)
			case .openingBrowser, .waitingForBrowser, .installing:
				setAccountReauthenticationPhase(
					.cancellationFailed(
						"The login is still active. Choose Cancel again."
					),
					sessionID: presentation.sessionID
				)
			}
		} catch {
			guard accountReauthentication?.sessionID == presentation.sessionID else {
				return
			}
			setAccountReauthenticationPhase(
				.cancellationFailed(Self.accountControlMessage(error)),
				sessionID: presentation.sessionID
			)
		}
	}

	func closeAccountReauthentication() {
		guard let presentation = accountReauthentication,
			presentation.canCloseWithoutCancellation
		else {
			return
		}
		finishAccountReauthentication(
			accountID: presentation.accountID,
			sessionID: presentation.sessionID
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
			activity: .route,
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
		message = nil
		setPendingStatus(.checking(detail: nil), for: attempt)
		submittingKey = attempt.idempotencyKey
		defer {
			submittingKey = nil
		}
		await inventoryReads.beginEffect(attempt.target.accountID)

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
			await inventoryReads.endEffect(attempt.target.accountID)
			reloadPendingJournal()
			if isPendingRecoveryBlocked {
				message = Self.pendingRecoveryBlockedMessage
			} else {
				setPendingStatus(
					.retrying(detail: Self.pendingDispatchUnavailableDetail),
					for: attempt
				)
			}
			return ResetCardUseCompletion(resolved: false)
		}

		let completion = await apply(dispatch, to: attempt)
		await inventoryReads.endEffect(attempt.target.accountID)
		return completion
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
		switch error {
		case .commandRejected:
			message = ResetCardStoreMessage(
				tone: .error,
				text: error.localizedDescription
			)
			if removeTerminalAttempt {
				forget(attempt)
			}
			await beginPostUseReconciliation(attempt.target.accountID)
			return ResetCardUseCompletion(resolved: true)
		case .nativeClientUnavailable, .timedOut, .outputTooLarge,
			.useDefinitelyNotDispatched, .usePotentiallyDispatched,
			.transportDisconnected, .transportBackpressured,
			.invalidResponse, .service:
			reloadPendingJournal()
			setPendingStatus(
				.retrying(detail: error.localizedDescription),
				for: attempt
			)
			return ResetCardUseCompletion(resolved: false)
		}
	}

	private func apply(
		_ state: ResetCardOperationState,
		to attempt: ResetCardUseAttempt,
		removeTerminalAttempt: Bool = true
	) async -> ResetCardUseCompletion {
		switch state {
		case .completed, .failedBeforeEffect:
			message = ResetCardStoreMessage(
				tone: Self.messageTone(for: state),
				text: state.presentation
			)
			if removeTerminalAttempt {
				forget(attempt)
			}
			await beginPostUseReconciliation(attempt.target.accountID)
			return ResetCardUseCompletion(resolved: true)
		case .prepared, .effectAmbiguous, .notFound, .unavailable:
			reloadPendingJournal()
			setPendingStatus(Self.pendingStatus(for: state), for: attempt)
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
					_ = await apply(state, to: attempt)
				case .unavailable(let error):
					shouldRetry = shouldRetry || error.isRetryableReadFailure
					_ = await apply(state, to: attempt)
				case .notFound:
					shouldRetry = true
					_ = await apply(state, to: attempt)
				}
			} catch {
				let clientError = Self.clientError(error)
				shouldRetry = shouldRetry || clientError.isRetryableReadFailure
				reloadPendingJournal()
				setPendingStatus(
					.retrying(detail: clientError.localizedDescription),
					for: attempt
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
		reconcilePendingStatuses()
		return true
	}

	private func forget(_ attempt: ResetCardUseAttempt) {
		guard isPendingRecoveryBlocked == false else {
			return
		}
		if let updated = pendingStore.remove(attempt) {
			pendingAttempts = updated
			reconcilePendingStatuses()
		}
	}

	private func reloadPendingJournal() {
		let load = pendingStore.load()
		pendingAttempts = load.attempts
		isPendingRecoveryBlocked = load.isRecoveryBlocked
		reconcilePendingStatuses()
	}

	private func setPendingStatus(
		_ status: ResetCardPendingStatus,
		for attempt: ResetCardUseAttempt
	) {
		guard pendingAttempts.contains(attempt) else {
			pendingStatuses.removeValue(forKey: attempt.idempotencyKey)
			return
		}
		pendingStatuses[attempt.idempotencyKey] = status
	}

	private func reconcilePendingStatuses() {
		let pendingKeys = Set(pendingAttempts.map(\.idempotencyKey))
		pendingStatuses = pendingStatuses.filter { pendingKeys.contains($0.key) }
		for attempt in pendingAttempts
		where pendingStatuses[attempt.idempotencyKey] == nil {
			pendingStatuses[attempt.idempotencyKey] = .checking(detail: nil)
		}
	}

	private func beginPostUseReconciliation(_ accountID: String) async {
		guard postUseReconciliationTasks[accountID] == nil else {
			return
		}

		// A terminal use result is an effect boundary. A read that started before
		// this point may finish, but it must not satisfy the post-effect refresh.
		await inventoryReads.invalidate(accountID)
		guard isPreparingForTermination == false,
			let index = accounts.firstIndex(where: {
				$0.account.accountID == accountID
			})
		else {
			return
		}

		postUseReconciliationAccountIDs.insert(accountID)
		let existing = accounts[index]
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: nil,
			isRefreshing: true,
			profile: existing.profile,
			profileUnavailable: existing.profileUnavailable,
			profileError: existing.profileError,
			isProfileRefreshing: existing.isProfileRefreshing
		)

		postUseReconciliationTasks[accountID] = Task { [weak self] in
			await self?.runPostUseReconciliation(accountID)
		}
	}

	private func runPostUseReconciliation(_ accountID: String) async {
		defer {
			postUseReconciliationTasks.removeValue(forKey: accountID)
		}

		var result = await refreshAccount(
			accountID,
			completesPostUseReconciliation: true
		)
		if await finishPostUseReconciliationStep(
			result,
			accountID: accountID
		) {
			return
		}

		for delay in postUseRetryDelays {
			do {
				try await Task.sleep(for: delay)
			} catch {
				return
			}
			guard Task.isCancelled == false else {
				return
			}
			guard postUseReconciliationAccountIDs.contains(accountID) else {
				return
			}

			result = await refreshAccount(
				accountID,
				completesPostUseReconciliation: true
			)
			if await finishPostUseReconciliationStep(
				result,
				accountID: accountID
			) {
				return
			}
		}
	}

	private func finishPostUseReconciliationStep(
		_ result: ResetCardInventoryRefreshResult,
		accountID: String
	) async -> Bool {
		switch result {
		case .current, .failed, .missing:
			return true
		case .awaitingSkeleton:
			await refreshAccountSkeleton()
			if canCompletePostUseReconciliation(accountID) {
				completePostUseReconciliation(accountID)
				return true
			}
			return postUseReconciliationAccountIDs.contains(accountID) == false
		case .retryNeeded:
			if isAwaitingFreshAccountSkeleton(accountID) {
				await refreshAccountSkeleton()
			}
			return postUseReconciliationAccountIDs.contains(accountID) == false
		}
	}

	private func canCompletePostUseReconciliation(_ accountID: String) -> Bool {
		guard let state = accounts.first(where: {
			$0.account.accountID == accountID
		}) else {
			return false
		}
		return state.inventoryIsCurrent
			&& state.error == nil
			&& state.inventory?.observationError == nil
	}

	private func completePostUseReconciliation(_ accountID: String) {
		guard postUseReconciliationAccountIDs.remove(accountID) != nil else {
			return
		}
		guard let index = accounts.firstIndex(where: {
			$0.account.accountID == accountID
		}) else {
			return
		}
		let existing = accounts[index]
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: existing.error,
			isRefreshing: isAwaitingFreshAccountSkeleton(accountID),
			profile: existing.profile,
			profileUnavailable: existing.profileUnavailable,
			profileError: existing.profileError,
			isProfileRefreshing: existing.isProfileRefreshing
		)
	}

	private func prunePostUseReconciliationsForCurrentAccounts() {
		let currentAccountIDs = Set(accounts.map(\.account.accountID))
		let removedAccountIDs = postUseReconciliationAccountIDs.subtracting(
			currentAccountIDs
		)
		guard removedAccountIDs.isEmpty == false else {
			return
		}

		for accountID in removedAccountIDs {
			postUseReconciliationAccountIDs.remove(accountID)
			postUseReconciliationTasks.removeValue(forKey: accountID)?.cancel()
			Task { [inventoryReads] in
				await inventoryReads.discard(accountID)
			}
		}
	}

	private func refreshAccount(
		_ accountID: String,
		completesPostUseReconciliation: Bool = false
	) async -> ResetCardInventoryRefreshResult {
		guard let index = accounts.firstIndex(where: { $0.account.accountID == accountID }) else {
			postUseReconciliationAccountIDs.remove(accountID)
			return .missing
		}

		let existing = accounts[index]
		do {
			let inventory = try await inventoryReads.inventory(for: existing.account)
			return applyInventory(
				inventory,
				accountID: accountID,
				completesPostUseReconciliation: completesPostUseReconciliation
			)
		} catch {
			let clientError = Self.clientError(error)
			applyInventoryFailure(
				clientError,
				accountID: accountID,
				completesPostUseReconciliation: completesPostUseReconciliation
			)
			return clientError.isRetryableReadFailure ? .retryNeeded : .failed
		}
	}

	private func refreshAccountDetails(
		_ accountID: String,
		refreshInventory: Bool = true
	) async {
		guard let index = accounts.firstIndex(where: {
			$0.account.accountID == accountID
		}) else {
			return
		}
		let account = accounts[index].account
		let inventoryReads = self.inventoryReads
		let includeEmail = true
			let profileRequest = accountProfileClient.map { _ in
				AccountProfileRequest(
					generation: beginProfileRequestGeneration(accountID: accountID),
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
			if refreshInventory {
				group.addTask {
					do {
						return .inventoryAvailable(
							accountID: accountID,
							try await inventoryReads.inventory(for: account)
						)
					} catch {
						return .inventoryFailed(
							accountID: accountID,
							Self.clientError(error)
						)
					}
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
		let refreshGeneration = accountSkeletonRefreshGeneration
		isRefreshingAccountSkeleton = true
		defer {
			isRefreshingAccountSkeleton = false
			if refreshGeneration != accountSkeletonRefreshGeneration {
				scheduleFreshAccountSkeletonRead()
			}
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
			var accountsNeedingDetails = [(
				accountID: String,
				refreshInventory: Bool
			)]()
			var postUseReconciledAccountIDs = Set<String>()
			accounts = snapshot.accounts.map { account in
				let previous = previousByID[account.accountID]
				let authority = snapshot.authority
					?? account.authority
					?? previous?.account.authority
					?? previous?.inventory?.authority
				let bound = Self.account(account, authority: authority)
				let sameRevision = previous?.account.accountRevision
					== bound.accountRevision
				let advancedInventory = advancedInventoriesAwaitingSkeleton[
					bound.accountID
				].flatMap { inventory in
					inventory.accountID == bound.accountID
						&& inventory.accountRevision == bound.accountRevision
						? inventory
						: nil
				}
				let reconcilesPostUse = advancedInventory != nil
					&& postUseReconciliationAccountIDs.contains(bound.accountID)
					&& postUseReconciliationTasks[bound.accountID] == nil
				if reconcilesPostUse {
					postUseReconciledAccountIDs.insert(bound.accountID)
				}
				let retainedInventory = advancedInventory ?? previous?.inventory
				let inventoryIsCurrent = retainedInventory.map {
					$0.accountID == bound.accountID
						&& $0.accountRevision == bound.accountRevision
				} ?? false
				let awaitsNewerSkeleton = accountSkeletonRevisionTargets[
					bound.accountID
				].map { bound.accountRevision < $0 } ?? false
				if sameRevision == false {
					accountsNeedingDetails.append(
						(
							accountID: bound.accountID,
							refreshInventory: advancedInventory == nil
								|| advancedInventory?.observationError != nil
						)
					)
				}
				return ResetCardAccountState(
					account: bound,
					inventory: retainedInventory,
					error: advancedInventory == nil && sameRevision
						? previous?.error
						: nil,
					isRefreshing: awaitsNewerSkeleton
						|| (sameRevision == false && inventoryIsCurrent == false)
						|| (
							postUseReconciliationAccountIDs.contains(bound.accountID)
								&& reconcilesPostUse == false
						),
					profile: sameRevision ? previous?.profile : nil,
					profileUnavailable: sameRevision
						? previous?.profileUnavailable
						: nil,
					profileError: sameRevision ? previous?.profileError : nil,
					isProfileRefreshing: sameRevision == false
					&& accountProfileClient != nil
				)
			}
			postUseReconciliationAccountIDs.subtract(
				postUseReconciledAccountIDs
			)
			prunePostUseReconciliationsForCurrentAccounts()
			pruneProfileEmailCache()
			reconcileAccountSkeletonRevisionTargets()
			pruneAdvancedInventoriesAwaitingSkeleton()
			for details in accountsNeedingDetails {
				scheduleAccountControlFollowUp(
					.account(
						details.accountID,
						refreshInventory: details.refreshInventory
					)
				)
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

	@discardableResult
	private func applyInventory(
		_ inventory: ResetCardInventory,
		accountID: String,
		completesPostUseReconciliation: Bool = false
	) -> ResetCardInventoryRefreshResult {
		if completesPostUseReconciliation == false,
			postUseReconciliationAccountIDs.contains(accountID),
			postUseReconciliationTasks[accountID] != nil
		{
			return .retryNeeded
		}
		guard inventory.accountID == accountID,
			let index = accounts.firstIndex(where: { $0.account.accountID == accountID })
		else {
			applyInventoryFailure(
				.invalidResponse,
				accountID: accountID,
				completesPostUseReconciliation: completesPostUseReconciliation
			)
			return .failed
		}
		let existing = accounts[index].account
		guard inventory.accountRevision >= existing.accountRevision else {
			return .retryNeeded
		}
		if let observationError = inventory.observationError {
			if inventory.accountRevision > existing.accountRevision {
				accountSkeletonRevisionTargets[accountID] = max(
					accountSkeletonRevisionTargets[accountID] ?? 0,
					inventory.accountRevision
				)
			}
			applyInventoryFailure(
				.service(observationError),
				accountID: accountID,
				completesPostUseReconciliation: completesPostUseReconciliation
			)
			if inventory.accountRevision > existing.accountRevision {
				scheduleFreshAccountSkeletonRead()
			}
			return observationError.isRetryableReadFailure
				? .retryNeeded
				: .failed
		}
		guard inventory.accountRevision == existing.accountRevision else {
			rejectAdvancedInventory(
				inventory,
				accountID: accountID,
				index: index
			)
			return .awaitingSkeleton
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
			isRefreshing: postUseReconciliationAccountIDs.contains(accountID)
				|| isAwaitingFreshAccountSkeleton(accountID),
			profile: retainsProfileState ? accounts[index].profile : nil,
			profileUnavailable: retainsProfileState
				? accounts[index].profileUnavailable
				: nil,
			profileError: retainsProfileState ? accounts[index].profileError : nil,
			isProfileRefreshing: accounts[index].isProfileRefreshing
		)
		if completesPostUseReconciliation
			|| postUseReconciliationTasks[accountID] == nil
		{
			completePostUseReconciliation(accountID)
		}
		if let deferred = advancedInventoriesAwaitingSkeleton[accountID],
			deferred.accountRevision <= inventory.accountRevision
		{
			advancedInventoriesAwaitingSkeleton.removeValue(forKey: accountID)
		}
		if revisionChanged {
			invalidateCodexProjectionAfterRevisionChange(
				accountID: accountID,
				accountRevision: inventory.accountRevision
			)
		}
		return .current
	}

	private func rejectAdvancedInventory(
		_ inventory: ResetCardInventory,
		accountID: String,
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
		if advancedInventoriesAwaitingSkeleton[accountID].map({
			$0.accountRevision <= inventory.accountRevision
		}) ?? true {
			advancedInventoriesAwaitingSkeleton[accountID] = inventory
		}

		accountSkeletonRevisionTargets[accountID] = max(
			accountSkeletonRevisionTargets[accountID] ?? 0,
			inventory.accountRevision
		)
		scheduleFreshAccountSkeletonRead()
	}

	private func scheduleFreshAccountSkeletonRead() {
		guard accountSkeletonRevisionTargets.isEmpty == false else {
			return
		}
		accountSkeletonRefreshGeneration &+= 1
		guard isAccountControlInProgress == false else {
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

	private func pruneAdvancedInventoriesAwaitingSkeleton() {
		let revisionsByID = Dictionary(
			uniqueKeysWithValues: accounts.map {
				($0.account.accountID, $0.account.accountRevision)
			}
		)
		advancedInventoriesAwaitingSkeleton =
			advancedInventoriesAwaitingSkeleton.filter { accountID, inventory in
				guard let accountRevision = revisionsByID[accountID] else {
					return false
				}
				return accountRevision < inventory.accountRevision
			}
	}

	private func applyInventoryFailure(
		_ error: ResetCardClientError,
		accountID: String,
		completesPostUseReconciliation: Bool = false
	) {
		guard completesPostUseReconciliation
			|| postUseReconciliationAccountIDs.contains(accountID) == false
			|| postUseReconciliationTasks[accountID] == nil
		else {
			return
		}
		guard let index = accounts.firstIndex(where: { $0.account.accountID == accountID }) else {
			return
		}
		let isRetryable = error.isRetryableReadFailure
		if isRetryable == false
			&& (
				completesPostUseReconciliation
					|| postUseReconciliationTasks[accountID] == nil
			)
		{
			postUseReconciliationAccountIDs.remove(accountID)
		}
		let existing = accounts[index]
		accounts[index] = ResetCardAccountState(
			account: existing.account,
			inventory: existing.inventory,
			error: error,
			isRefreshing: isRetryable
				&& (
					postUseReconciliationAccountIDs.contains(accountID)
						|| isAwaitingFreshAccountSkeleton(accountID)
				),
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
			let index = accounts.firstIndex(where: {
				$0.account.accountID == accountID
			})
		else {
			return
		}
		let read = ResetCardAccountRead.profileAvailable(
			accountID: accountID,
			request: request,
			profile
		)
		let existing = accounts[index]
		let updated = profileState(
			existing,
			applying: read,
			includesEmail: profileEmailsVisible
		)
		cacheEmail(from: read, replacing: existing, with: updated)
		accounts[index] = updated
		_ = publishProfileEmailsIfReady(expectedEpoch: profilePrivacyEpoch)
	}

	private func applyProfileUnavailable(
		_ unavailable: AccountProfileUnavailable,
		accountID: String,
		request: AccountProfileRequest
	) {
		guard isCurrentProfileRequest(request),
			let index = accounts.firstIndex(where: {
				$0.account.accountID == accountID
			})
		else {
			return
		}
		let read = ResetCardAccountRead.profileUnavailable(
			accountID: accountID,
			request: request,
			unavailable
		)
		let existing = accounts[index]
		let updated = profileState(
			existing,
			applying: read,
			includesEmail: profileEmailsVisible
		)
		cacheEmail(from: read, replacing: existing, with: updated)
		accounts[index] = updated
		_ = publishProfileEmailsIfReady(expectedEpoch: profilePrivacyEpoch)
	}

	private func applyProfileFailure(
		_ error: ResetCardClientError,
		accountID: String,
		request: AccountProfileRequest
	) {
		guard isCurrentProfileRequest(request),
			let index = accounts.firstIndex(where: {
				$0.account.accountID == accountID
			})
		else {
			return
		}
		accounts[index] = profileState(
			accounts[index],
			applying: .profileFailed(
				accountID: accountID,
				request: request,
				error
			),
			includesEmail: profileEmailsVisible
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
		let accountSnapshot = accounts.filter {
			hasCachedEmail(for: $0) == false
		}
		guard accountSnapshot.isEmpty == false else {
			return
		}
		var requests = [AccountProfileRequest]()
		defer {
			for request in requests {
				finishProfileRequest(request)
			}
		}
		await withTaskGroup(of: ResetCardAccountRead.self) { group in
			for state in accountSnapshot {
				let account = state.account
				let request = AccountProfileRequest(
					generation: beginProfileRequestGeneration(
						accountID: account.accountID
					),
					includesEmail: true,
					accountID: account.accountID,
					accountRevision: account.accountRevision
				)
				requests.append(request)
				group.addTask {
					do {
						switch try await accountProfileClient.profile(
							for: account,
							includeEmail: true
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
				case .profileAvailable(let accountID, let request, let profile):
					applyProfile(profile, accountID: accountID, request: request)
				case .profileUnavailable(let accountID, let request, let unavailable):
					applyProfileUnavailable(
						unavailable,
						accountID: accountID,
						request: request
					)
				case .profileFailed(let accountID, let request, let error):
					applyProfileFailure(error, accountID: accountID, request: request)
				case .inventoryAvailable, .inventoryFailed:
					break
				}
			}
		}
	}

	@discardableResult
	private func publishProfileEmailsIfReady(expectedEpoch: UInt64) -> Bool {
		guard requestedProfileEmailVisibility,
			profileEmailsVisible == false,
			profilePrivacyEpoch == expectedEpoch,
			accounts.isEmpty == false,
			accounts.allSatisfy(hasCachedEmail)
		else {
			return false
		}

		// Identity changes are one whole-array publication. Profile metrics and
		// errors continue to arrive independently while this reveal is pending.
		profileEmailsVisible = true
		accounts = accounts.map(revealingCachedEmail)
		return true
	}

	private func hasCachedEmail(for state: ResetCardAccountState) -> Bool {
		profileEmailCache[state.account.accountID]?.accountRevision
			== state.account.accountRevision
	}

	private func revealingCachedEmail(
		_ state: ResetCardAccountState
	) -> ResetCardAccountState {
		guard let cached = profileEmailCache[state.account.accountID],
			cached.accountRevision == state.account.accountRevision
		else {
			return state
		}
		return ResetCardAccountState(
			account: state.account,
			inventory: state.inventory,
			error: state.error,
			isRefreshing: state.isRefreshing,
			profile: state.profile?.replacingEmail(cached.email),
			profileUnavailable: state.profileUnavailable?.replacingEmail(cached.email),
			profileError: state.profileError,
			isProfileRefreshing: state.isProfileRefreshing
		)
	}

	private func cacheEmail(
		from read: ResetCardAccountRead,
		replacing existing: ResetCardAccountState,
		with updated: ResetCardAccountState
	) {
		guard updated.profileError != .invalidResponse else {
			return
		}
		switch read {
		case .profileAvailable(let accountID, let request, let profile):
			guard request.includesEmail,
				accountID == existing.account.accountID,
				profile.accountID == accountID,
				profile.accountRevision == existing.account.accountRevision,
				Self.canReplaceRetainedProfile(
					existing.profile,
					with: profile,
					allowsEmailEnrichment: true
				)
			else {
				return
			}
			profileEmailCache[accountID] = CachedAccountEmail(
				accountRevision: profile.accountRevision,
				email: profile.email
			)
		case .profileUnavailable(let accountID, let request, let unavailable):
			guard request.includesEmail,
				accountID == existing.account.accountID,
				request.accountRevision == existing.account.accountRevision
			else {
				return
			}
			profileEmailCache[accountID] = CachedAccountEmail(
				accountRevision: request.accountRevision,
				email: unavailable.claims.email
			)
		case .profileFailed, .inventoryAvailable, .inventoryFailed:
			break
		}
	}

	private func pruneProfileEmailCache() {
		let revisions = Dictionary(
			uniqueKeysWithValues: accounts.map {
				($0.account.accountID, $0.account.accountRevision)
			}
		)
		profileEmailCache = profileEmailCache.filter { accountID, cached in
			revisions[accountID] == cached.accountRevision
		}
	}

	private func profileState(
		_ state: ResetCardAccountState,
		applying read: ResetCardAccountRead,
		includesEmail: Bool
	) -> ResetCardAccountState {
		switch read {
		case .profileAvailable(let accountID, let request, let profile):
			guard request.accountID == accountID,
				accountID == state.account.accountID,
				profile.accountID == accountID,
				profile.accountRevision == state.account.accountRevision
			else {
				return profileFailureState(state, error: .invalidResponse)
			}
			let candidate = includesEmail ? profile : profile.redactingEmail()
			guard Self.canReplaceRetainedProfile(
				state.profile,
				with: candidate,
				allowsEmailEnrichment: includesEmail
			) else {
				return profileFailureState(state, error: .invalidResponse)
			}
			return ResetCardAccountState(
				account: state.account,
				inventory: state.inventory,
				error: state.error,
				isRefreshing: state.isRefreshing,
				profile: candidate,
				profileUnavailable: nil,
				profileError: nil,
				isProfileRefreshing: false
			)
		case .profileUnavailable(let accountID, let request, let unavailable):
			guard request.accountID == accountID,
				accountID == state.account.accountID,
				request.accountRevision == state.account.accountRevision
			else {
				return profileFailureState(state, error: .invalidResponse)
			}
			return ResetCardAccountState(
				account: state.account,
				inventory: state.inventory,
				error: state.error,
				isRefreshing: state.isRefreshing,
				profile: state.profile,
				profileUnavailable: includesEmail
					? unavailable
					: unavailable.redactingEmail(),
				profileError: nil,
				isProfileRefreshing: false
			)
		case .profileFailed(let accountID, let request, let error):
			guard request.accountID == accountID,
				accountID == state.account.accountID,
				request.accountRevision == state.account.accountRevision
			else {
				return profileFailureState(state, error: .invalidResponse)
			}
			return profileFailureState(state, error: error)
		case .inventoryAvailable, .inventoryFailed:
			return state
		}
	}

	private func profileFailureState(
		_ state: ResetCardAccountState,
		error: ResetCardClientError
	) -> ResetCardAccountState {
		ResetCardAccountState(
			account: state.account,
			inventory: state.inventory,
			error: state.error,
			isRefreshing: state.isRefreshing,
			profile: state.profile,
			profileUnavailable: nil,
			profileError: error,
			isProfileRefreshing: false
		)
	}

	private func beginProfileRequestGeneration(accountID: String) -> UInt64 {
		let generation = (profileRequestGenerations[accountID] ?? 0) &+ 1
		profileRequestGenerations[accountID] = generation
		return generation
	}

	private func isCurrentProfileRequest(_ request: AccountProfileRequest) -> Bool {
		guard request.generation == profileRequestGenerations[request.accountID],
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

	private func runAccountReauthentication(
		client: any AccountControlClient,
		authority: ResetCardAuthority?,
		account: ResetCardAccountRecord,
		sessionID: String,
		operationID: String,
		idempotencyKey: String
	) async {
		do {
			let codexBin = try resolveCodexExecutable()
			guard accountReauthentication?.sessionID == sessionID else {
				return
			}
			setAccountReauthenticationPhase(
				.openingBrowser,
				sessionID: sessionID
			)
			var status = try await client.startAccountReauthentication(
				authority: authority,
				sessionID: sessionID,
				operationID: operationID,
				accountID: account.accountID,
				expectedRevision: account.accountRevision,
				idempotencyKey: idempotencyKey,
				codexBin: codexBin
			)

			while Task.isCancelled == false,
				accountReauthentication?.sessionID == sessionID
			{
				if await applyAccountReauthenticationStatus(
					status,
					accountID: account.accountID,
					sessionID: sessionID
				) {
					return
				}
				try await Task.sleep(for: accountReauthenticationPollInterval)
				status = try await client.pollAccountReauthentication(
					authority: authority,
					sessionID: sessionID
				)
			}
		} catch is CancellationError {
			return
		} catch {
			guard accountReauthentication?.sessionID == sessionID else {
				return
			}
			if let cancellationStatus = try? await client.cancelAccountReauthentication(
				authority: authority,
				sessionID: sessionID
			),
				await applyAccountReauthenticationStatus(
					cancellationStatus,
					accountID: account.accountID,
					sessionID: sessionID
				)
			{
				return
			}
			await failAccountReauthentication(
				accountID: account.accountID,
				sessionID: sessionID,
				message: Self.accountControlMessage(error)
			)
		}
	}

	private func applyAccountReauthenticationStatus(
		_ status: AccountReauthenticationStatus,
		accountID: String,
		sessionID: String
	) async -> Bool {
		guard status.sessionID == sessionID,
			accountReauthentication?.sessionID == sessionID
		else {
			await failAccountReauthentication(
				accountID: accountID,
				sessionID: sessionID,
				message: AccountControlError.invalidResponse.localizedDescription
			)
			return true
		}

		switch status.state {
		case .openingBrowser:
			setAccountReauthenticationPhase(
				.openingBrowser,
				sessionID: sessionID
			)
			return false
		case .waitingForBrowser:
			setAccountReauthenticationPhase(
				.waitingForBrowser,
				sessionID: sessionID
			)
			return false
		case .installing:
			setAccountReauthenticationPhase(
				.installing,
				sessionID: sessionID
			)
			return false
		case .completed:
			await completeAccountReauthentication(
				accountID: accountID,
				sessionID: sessionID
			)
			return true
		case .failed:
			await failAccountReauthentication(
				accountID: accountID,
				sessionID: sessionID,
				message: status.failure?.presentation
					?? AccountControlError.invalidResponse.localizedDescription,
				failure: status.failure
			)
			return true
		case .cancelled:
			finishAccountReauthentication(
				accountID: accountID,
				sessionID: sessionID
			)
			return true
		}
	}

	private func setAccountReauthenticationPhase(
		_ phase: AccountReauthenticationPhase,
		sessionID: String
	) {
		guard let presentation = accountReauthentication,
			presentation.sessionID == sessionID
		else {
			return
		}
		accountReauthentication = AccountReauthenticationPresentation(
			accountID: presentation.accountID,
			accountLabel: presentation.accountLabel,
			sessionID: sessionID,
			authority: presentation.authority,
			phase: phase
		)
	}

	private func completeAccountReauthentication(
		accountID: String,
		sessionID: String
	) async {
		guard accountReauthentication?.sessionID == sessionID else {
			return
		}
		accountControlActivities.removeValue(forKey: accountID)
		message = ResetCardStoreMessage(
			tone: .success,
			text: "Account login refreshed."
		)
		accountReauthentication = nil
		await refreshReauthenticatedAccountAuthority(
			accountID,
			retryDelays: accountObservationRetryDelays
		)
		accountReauthenticationTask = nil
	}

	private func refreshReauthenticatedAccountAuthority(
		_ accountID: String,
		retryDelays: [Duration] = []
	) async {
		await refreshReauthenticatedAccountAuthorityOnce(accountID)
		for delay in retryDelays {
			guard Task.isCancelled == false,
				accounts.first(where: {
					$0.account.accountID == accountID
				})?.requiresLoginRefresh == true
			else {
				return
			}
			do {
				try await Task.sleep(for: delay)
			} catch {
				return
			}
			await refreshReauthenticatedAccountAuthorityOnce(accountID)
		}
	}

	private func refreshReauthenticatedAccountAuthorityOnce(_ accountID: String) async {
		await refreshAccountSkeleton()
		await refreshAccountDetails(accountID)
		await refreshAccountSkeleton()
	}

	private func failAccountReauthentication(
		accountID: String,
		sessionID: String,
		message: String,
		failure: AccountReauthenticationFailure? = nil
	) async {
		guard accountReauthentication?.sessionID == sessionID else {
			return
		}
		accountControlActivities.removeValue(forKey: accountID)
		accountReauthenticationTask = nil
		setAccountReauthenticationPhase(
			.failed(message),
			sessionID: sessionID
		)
		if failure == .outcomeUnknown {
			await refreshReauthenticatedAccountAuthority(accountID)
		}
	}

	private func finishAccountReauthentication(
		accountID: String,
		sessionID: String
	) {
		guard accountReauthentication?.sessionID == sessionID else {
			return
		}
		accountReauthenticationTask?.cancel()
		accountReauthenticationTask = nil
		accountControlActivities.removeValue(forKey: accountID)
		accountReauthentication = nil
	}

	nonisolated private static func accountControlMessage(_ error: Error) -> String {
		if let error = error as? LocalizedError,
			let description = error.errorDescription
		{
			return description
		}
		return AccountControlError.invalidResponse.localizedDescription
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
				isRoutingControl == false || accountControlActivities.isEmpty,
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
		}
		if isRoutingControl {
			isRoutingAccountControl = true
		}
		clearStaleControlError()
		defer {
			if let accountID {
				accountControlActivities.removeValue(forKey: accountID)
			}
			if isRoutingControl {
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
		case account(String, refreshInventory: Bool)
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
			_ = reorderAccountStates(to: routing.order)
			return .none
		case .accountLoggedOut(let accountID, _):
			if case .current(let projectedID, _, _) = codexAuthProjection,
				projectedID == accountID
			{
				codexProjectionRequestGeneration &+= 1
				codexAuthProjection = nil
			}
			accounts.removeAll { $0.account.accountID == accountID }
			postUseReconciliationAccountIDs.remove(accountID)
			postUseReconciliationTasks.removeValue(forKey: accountID)?.cancel()
			accountSkeletonRevisionTargets.removeValue(forKey: accountID)
			advancedInventoriesAwaitingSkeleton.removeValue(forKey: accountID)
			Task { [inventoryReads] in
				await inventoryReads.discard(accountID)
			}
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
				inventory: existing.inventory,
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
			return sameRevision
				? .none
				: .account(account.accountID, refreshInventory: true)
		}
	}

	@discardableResult
	private func reorderAccountStates(to order: [String]) -> Bool {
		guard order.count == accounts.count else {
			return false
		}
		let statesByID = Dictionary(
			uniqueKeysWithValues: accounts.map {
				($0.account.accountID, $0)
			}
		)
		guard statesByID.count == accounts.count,
			Set(statesByID.keys) == Set(order),
			Set(order).count == order.count
		else {
			return false
		}
		accounts = order.compactMap { statesByID[$0] }
		return true
	}

	private func scheduleAccountControlFollowUp(
		_ followUp: AccountControlFollowUp
	) {
		switch followUp {
		case .none:
			return
		case .account(let accountID, let refreshInventory):
			Task { [weak self] in
				await self?.refreshAccountDetails(
					accountID,
					refreshInventory: refreshInventory
				)
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

	private static func pendingStatus(
		for state: ResetCardOperationState
	) -> ResetCardPendingStatus {
		switch state {
		case .prepared:
			return .checking(detail: "The service accepted this Reset Card request.")
		case .effectAmbiguous:
			return .checking(
				detail: "The service is reconciling authoritative Reset Card state."
			)
		case .notFound:
			return .checking(
				detail: "No durable Reset Card operation was found yet."
			)
		case .unavailable(let error):
			return .retrying(detail: error.presentation)
		case .completed, .failedBeforeEffect:
			return .checking(detail: nil)
		}
	}

	private static let pendingRecoveryBlockedMessage = ResetCardStoreMessage(
		tone: .error,
		text: "The pending reset-card recovery journal is invalid or unavailable. New use is blocked. Preserve the journal for manual inspection; no automatic repair is available."
	)

	private static let pendingDispatchUnavailableDetail =
		"Another app instance changed or is checking this saved Reset Card request."

	private static let pendingTerminalRemovalFailedMessage = ResetCardStoreMessage(
		tone: .error,
		text: "The Reset Card operation finished, but the recovery journal could not be updated. Preserve the journal and refresh before starting another request."
	)
}

private extension ResetCardClientError {
	var isConnectionFailure: Bool {
		switch self {
		case .transportDisconnected:
			return true
		case .nativeClientUnavailable, .timedOut, .transportBackpressured,
			.outputTooLarge, .commandRejected, .useDefinitelyNotDispatched,
			.usePotentiallyDispatched, .invalidResponse, .service:
			return false
		}
	}

	var isRetryableReadFailure: Bool {
		switch self {
		case .nativeClientUnavailable, .timedOut, .transportDisconnected,
			.transportBackpressured:
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
			.inventoryIncomplete, .inventoryChanged, .requestTimedOut, .resourceExhausted,
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
