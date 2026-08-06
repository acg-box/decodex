@testable import DecodexApp
import Foundation
import XCTest

@MainActor
final class AccountProfileStoreTests: XCTestCase {
	func testProfileEmailVisibilityUsesRevisionBoundMemoryCache() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = ProfileStoreClient()
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()

		XCTAssertNotNil(store.accounts.first?.inventory)
		XCTAssertEqual(store.accounts.first?.profile?.snapshot.lifetimeTokens, 9_001)
		XCTAssertNil(store.accounts.first?.profile?.email)

		await store.setProfileEmailVisibility(true)
		XCTAssertEqual(store.accounts.first?.profile?.email, "iris@example.com")

		await store.setProfileEmailVisibility(false)
		XCTAssertNil(store.accounts.first?.profile?.email)
		XCTAssertEqual(store.accounts.first?.profile?.snapshot.lifetimeTokens, 9_001)
		let emailRequests = await client.emailRequests()
		XCTAssertEqual(emailRequests, [true])
	}

	func testVisibleEmailStartupPublishesOnlyAfterEveryAccountReadSettles() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = AtomicEmailProfileClient()
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.setProfileEmailVisibility(true)
		let refresh = Task {
			await store.refresh()
		}
		try await client.waitForVisibleRequests()
		XCTAssertEqual(store.accounts.count, 2)
		XCTAssertTrue(store.accounts.allSatisfy { $0.profile?.email == nil })

		await client.resolveVisibleProfile(accountID: AtomicEmailProfileClient.firstAccountID)
		for _ in 0 ..< 20 {
			await Task.yield()
		}
		let first = store.accounts.first {
			$0.account.accountID == AtomicEmailProfileClient.firstAccountID
		}
		let second = store.accounts.first {
			$0.account.accountID == AtomicEmailProfileClient.secondAccountID
		}
		XCTAssertEqual(first?.profile?.snapshot.lifetimeTokens, 1_000)
		XCTAssertNil(second?.profile)
		XCTAssertTrue(
			store.accounts.allSatisfy { $0.profile?.email == nil },
			"Startup must not expose only the first completed account email."
		)

		await client.resolveVisibleProfile(accountID: AtomicEmailProfileClient.secondAccountID)
		await refresh.value

		XCTAssertEqual(
			store.accounts.compactMap { $0.profile?.email }.sorted(),
			["first@example.com", "second@example.com"]
		)
	}

	func testEmailVisibilityWaitsForMissingCacheBeforePublishingAnyEmail() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = AtomicEmailProfileClient(mode: .cacheFirstAndRetrySecond)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		XCTAssertEqual(store.accounts.count, 2)
		XCTAssertTrue(store.accounts.allSatisfy { $0.profile?.email == nil })

		let showing = Task {
			await store.setProfileEmailVisibility(true)
		}
		try await client.waitForVisibleRequests(count: 1)
		XCTAssertTrue(
			store.accounts.allSatisfy { $0.profile?.email == nil },
			"A partial cache must not expose only one account email."
		)

		await client.resolveVisibleProfile(accountID: AtomicEmailProfileClient.secondAccountID)
		await showing.value

		XCTAssertEqual(
			store.accounts.compactMap { $0.profile?.email }.sorted(),
			["first@example.com", "second@example.com"]
		)
	}

	func testProfileFailureDoesNotRemoveResetCardInventory() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = ProfileStoreClient(
			profileResult: .unavailable(
				AccountProfileUnavailable(
					error: .providerUnavailable,
					claims: AccountProfileClaims(email: nil, planType: "pro")
				)
			)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()

		XCTAssertNotNil(store.accounts.first?.inventory)
		XCTAssertNil(store.accounts.first?.profile)
		XCTAssertEqual(
			store.accounts.first?.profileUnavailable?.error,
			.providerUnavailable
		)
		XCTAssertFalse(store.accounts.first?.requiresLoginRefresh ?? true)
		XCTAssertNil(store.accounts.first?.error)
	}

	func testCachedProfileUnauthorizedRequiresLoginRefresh() {
		let profile = profileObservation(
			observedAt: 300,
			lifetimeTokens: 3_000,
			freshness: .cached(refreshError: .unauthorized)
		)
		let availableState = ResetCardAccountState(
			account: profileTestAccount(),
			inventory: nil,
			error: nil,
			isRefreshing: false,
			profile: profile
		)

		XCTAssertTrue(availableState.requiresLoginRefresh)
		XCTAssertEqual(
			availableState.profileDegradationText,
			AccountProfileObservationError.unauthorized.presentation
		)

		let authFailedState = ResetCardAccountState(
			account: profileTestAccount(observedState: .authFailed),
			inventory: nil,
			error: nil,
			isRefreshing: false,
			profile: profile
		)
		XCTAssertTrue(authFailedState.requiresLoginRefresh)
	}

	func testUnavailableUnauthorizedRequiresLoginRefresh() {
		let state = ResetCardAccountState(
			account: profileTestAccount(),
			inventory: nil,
			error: nil,
			isRefreshing: false,
			profileUnavailable: AccountProfileUnavailable(
				error: .unauthorized,
				claims: AccountProfileClaims(email: nil, planType: "pro")
			)
		)

		XCTAssertTrue(state.requiresLoginRefresh)
	}

	func testOlderObservationCannotReplaceNewerRetainedProfile() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = SequencedProfileStoreClient(
			results: [
				.available(profileObservation(observedAt: 300, lifetimeTokens: 3_000)),
				.available(profileObservation(observedAt: 200, lifetimeTokens: 2_000)),
			]
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.refresh()

		XCTAssertEqual(store.accounts.first?.profile?.observedAtUnixMicros, 300)
		XCTAssertEqual(store.accounts.first?.profile?.snapshot.lifetimeTokens, 3_000)
		XCTAssertEqual(store.accounts.first?.profileError, .invalidResponse)
		XCTAssertNotNil(store.accounts.first?.profileDegradationText)
	}

	func testSameTimestampContentDriftCannotReplaceRetainedProfile() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = SequencedProfileStoreClient(
			results: [
				.available(profileObservation(observedAt: 300, lifetimeTokens: 3_000)),
				.available(profileObservation(observedAt: 300, lifetimeTokens: 9_000)),
			]
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.refresh()

		XCTAssertEqual(store.accounts.first?.profile?.snapshot.lifetimeTokens, 3_000)
		XCTAssertEqual(store.accounts.first?.profileError, .invalidResponse)
	}

	func testNewestEmailGenerationWinsAcrossRapidVisibilityChanges() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = SuspendedProfileStoreClient()
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let initialRefresh = Task { await store.refresh() }
		try await waitForProfileRequests(1, client: client)

		let firstShow = Task {
			await store.setProfileEmailVisibility(true)
		}
		try await waitForProfileRequests(2, client: client)

		await store.setProfileEmailVisibility(false)
		let secondShow = Task {
			await store.setProfileEmailVisibility(true)
		}
		try await waitForProfileRequests(3, client: client)

		await client.resolve(
			request: 2,
			with: .available(
				profileObservation(
					observedAt: 300,
					lifetimeTokens: 3_000,
					email: "new@example.com"
				)
			)
		)
		await secondShow.value
		await client.resolve(
			request: 1,
			with: .available(
				profileObservation(
					observedAt: 200,
					lifetimeTokens: 2_000,
					email: "old@example.com"
				)
			)
		)
		await client.resolve(
			request: 0,
			with: .available(
				profileObservation(observedAt: 100, lifetimeTokens: 1_000)
			)
		)
		await firstShow.value
		await initialRefresh.value

		XCTAssertEqual(store.accounts.first?.profile?.observedAtUnixMicros, 300)
		XCTAssertEqual(store.accounts.first?.profile?.snapshot.lifetimeTokens, 3_000)
		XCTAssertEqual(store.accounts.first?.profile?.email, "new@example.com")
		XCTAssertEqual(store.accounts.first?.isProfileRefreshing, false)
		let emailVisibility = await client.requestedEmailVisibility()
		XCTAssertEqual(emailVisibility, [true, true, true])
	}

	func testAdvancedInventoryPublishesBeforeStaleProfileReturns() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = SuspendedProfileStoreClient(inventoryRevision: 8)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refresh = Task { await store.refresh() }
		try await waitForProfileRequests(1, client: client)
		for _ in 0 ..< 2_000 {
			if store.accounts.first?.account.accountRevision == 8,
				store.accounts.first?.inventory != nil
			{
				break
			}
			await Task.yield()
		}
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertNotNil(store.accounts.first?.inventory)
		await client.resolve(
			request: 0,
			with: .unavailable(
				AccountProfileUnavailable(
					error: .providerUnavailable,
					claims: AccountProfileClaims(email: nil, planType: "pro")
				)
			)
		)
		await refresh.value

		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertNil(store.accounts.first?.profileUnavailable)
		XCTAssertNil(store.accounts.first?.profileError)
		XCTAssertEqual(store.accounts.first?.isProfileRefreshing, false)
	}

	func testAdvancedInventoryDropsProfileBoundToOlderRevision() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = RevisionAdvanceAfterUnavailableClient()
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refresh = Task { await store.refresh() }
		for _ in 0 ..< 2_000 {
			if store.accounts.first?.profileUnavailable != nil,
				await client.isInventoryWaiting()
			{
				break
			}
			await Task.yield()
		}
		XCTAssertEqual(
			store.accounts.first?.profileUnavailable?.claims.planType,
			"old-plan"
		)
		let isInventoryWaiting = await client.isInventoryWaiting()
		XCTAssertTrue(isInventoryWaiting)

		await client.resolveInventory()
		await refresh.value

		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertNotNil(store.accounts.first?.inventory)
		XCTAssertNil(store.accounts.first?.profile)
		XCTAssertNil(store.accounts.first?.profileUnavailable)
		XCTAssertNil(store.accounts.first?.profileError)
		XCTAssertEqual(store.accounts.first?.isProfileRefreshing, false)
	}

	func testSingleAccountFollowUpDoesNotInvalidateAnotherAccountProfile() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = PerAccountGenerationClient()
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let fullRefresh = Task { await store.refresh() }
		for _ in 0 ..< 2_000 {
			if await client.secondProfileIsPending() {
				break
			}
			await Task.yield()
		}
		let secondProfileIsPending = await client.secondProfileIsPending()
		XCTAssertTrue(secondProfileIsPending)

		await store.setAccount(PerAccountGenerationClient.firstAccountID, enabled: false)
		for _ in 0 ..< 2_000 {
			let first = store.accounts.first {
				$0.account.accountID == PerAccountGenerationClient.firstAccountID
			}
			if first?.account.accountRevision == 8,
				first?.profile?.snapshot.lifetimeTokens == 8_000
			{
				break
			}
			await Task.yield()
		}

		await client.releaseSecondProfile()
		await fullRefresh.value

		let first = try XCTUnwrap(store.accounts.first {
			$0.account.accountID == PerAccountGenerationClient.firstAccountID
		})
		let second = try XCTUnwrap(store.accounts.first {
			$0.account.accountID == PerAccountGenerationClient.secondAccountID
		})
		XCTAssertEqual(first.account.accountRevision, 8)
		XCTAssertEqual(first.profile?.snapshot.lifetimeTokens, 8_000)
		XCTAssertEqual(second.profile?.snapshot.lifetimeTokens, 2_000)
		XCTAssertFalse(second.isProfileRefreshing)
	}

	func testConcurrentAccountFollowUpCannotPublishOneEmailDuringReveal() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = PerAccountGenerationClient(mode: .failThenBlockSecond)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		XCTAssertTrue(store.accounts.allSatisfy { $0.profile?.email == nil })

		let reveal = Task {
			await store.setProfileEmailVisibility(true)
		}
		try await client.waitForSecondProfile()

		await store.setAccount(PerAccountGenerationClient.firstAccountID, enabled: false)
		for _ in 0 ..< 2_000 {
			let first = store.accounts.first {
				$0.account.accountID == PerAccountGenerationClient.firstAccountID
			}
			if first?.account.accountRevision == 8,
				first?.profile?.snapshot.lifetimeTokens == 8_000
			{
				break
			}
			await Task.yield()
		}

		XCTAssertFalse(store.profileEmailsVisible)
		XCTAssertTrue(
			store.accounts.allSatisfy { $0.profile?.email == nil },
			"A per-account follow-up must not publish one identity during a whole-list reveal."
		)

		await client.releaseSecondProfile()
		await reveal.value

		XCTAssertTrue(store.profileEmailsVisible)
		XCTAssertEqual(
			store.accounts.compactMap { $0.profile?.email }.sorted(),
			["first@example.com", "second@example.com"]
		)
	}

	func testSupersededGenerationKeepsUnrelatedProgressiveProfileResult() async throws {
		let fixture = try PendingFixture()
		defer { fixture.remove() }
		let client = PerAccountGenerationClient(mode: .blockInitialFirst)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.setProfileEmailVisibility(true)
		let refresh = Task {
			await store.refresh()
		}
		try await client.waitForFirstProfile()
		for _ in 0 ..< 2_000 {
			let second = store.accounts.first {
				$0.account.accountID == PerAccountGenerationClient.secondAccountID
			}
			if second?.profile?.snapshot.lifetimeTokens == 2_000 {
				break
			}
			await Task.yield()
		}

		let progressiveSecond = store.accounts.first {
			$0.account.accountID == PerAccountGenerationClient.secondAccountID
		}
		XCTAssertEqual(progressiveSecond?.profile?.snapshot.lifetimeTokens, 2_000)
		XCTAssertNil(progressiveSecond?.profile?.email)

		await store.setAccount(PerAccountGenerationClient.firstAccountID, enabled: false)
		for _ in 0 ..< 2_000 {
			let first = store.accounts.first {
				$0.account.accountID == PerAccountGenerationClient.firstAccountID
			}
			if first?.account.accountRevision == 8,
				first?.profile?.snapshot.lifetimeTokens == 8_000
			{
				break
			}
			await Task.yield()
		}

		let retainedSecond = store.accounts.first {
			$0.account.accountID == PerAccountGenerationClient.secondAccountID
		}
		XCTAssertEqual(retainedSecond?.profile?.snapshot.lifetimeTokens, 2_000)
		XCTAssertTrue(store.profileEmailsVisible)
		XCTAssertEqual(
			store.accounts.compactMap { $0.profile?.email }.sorted(),
			["first@example.com", "second@example.com"]
		)

		await client.releaseFirstProfile()
		await refresh.value

		let settledSecond = store.accounts.first {
			$0.account.accountID == PerAccountGenerationClient.secondAccountID
		}
		XCTAssertEqual(settledSecond?.profile?.snapshot.lifetimeTokens, 2_000)
	}

	private func waitForProfileRequests(
		_ expectedCount: Int,
		client: SuspendedProfileStoreClient
	) async throws {
		for _ in 0 ..< 2_000 {
			if await client.requestCount() >= expectedCount {
				return
			}
			await Task.yield()
		}
		throw WaitError.timedOut
	}
}

private enum WaitError: Error {
	case timedOut
}

private actor ProfileStoreClient: ResetCardClient, AccountProfileClient {
	private let profileResult: AccountProfileRead?
	private var requestedEmailVisibility = [Bool]()

	init(profileResult: AccountProfileRead? = nil) {
		self.profileResult = profileResult
	}

	func accounts(
		authority _: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[Self.account]
	}

	func inventory(
		for _: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		Self.inventory
	}

	func profile(
		for account: ResetCardAccountRecord,
		includeEmail: Bool
	) async throws -> AccountProfileRead {
		requestedEmailVisibility.append(includeEmail)
		if let profileResult {
			return profileResult
		}
		return .available(
			AccountProfileObservation(
				accountID: account.accountID,
				accountRevision: account.accountRevision,
				observedAtUnixMicros: 1_785_276_000_000_000,
				email: includeEmail ? "iris@example.com" : nil,
				planType: "pro",
				displayName: "Iris",
				username: "iris",
				snapshot: AccountProfileSnapshot(
					lifetimeTokens: 9_001,
					peakDailyTokens: 4_000,
					longestTaskSeconds: 125,
					currentStreakDays: 3,
					longestStreakDays: 8,
					dailyUsage: [
						AccountProfileDailyUsage(date: "2026-07-28", tokens: 4_000),
					]
				),
				freshness: .current
			)
		)
	}

	func use(
		_: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(
		for _: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func emailRequests() -> [Bool] {
		requestedEmailVisibility
	}

	private static let authority = ResetCardAuthority(
		profileName: "local",
		serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	)

	private static let account = ResetCardAccountRecord(
		authority: authority,
		accountID: "11111111-1111-4111-8111-111111111111",
		alias: "Account 00000-00001",
		accountRevision: 7,
		enabled: true,
		observedState: .available,
		lifecycleReadiness: .ready,
		fiveHourQuota: .unknown(durationMinutes: 300),
		sevenDayQuota: .unknown(durationMinutes: 10_080)
	)

	private static let inventory = ResetCardInventory(
		authority: authority,
		accountID: account.accountID,
		accountRevision: account.accountRevision,
		cards: [],
		fiveHourQuota: .unknown(durationMinutes: 300),
		sevenDayQuota: .unknown(durationMinutes: 10_080),
		observationError: nil
	)
}

private actor SequencedProfileStoreClient: ResetCardClient, AccountProfileClient {
	private var results: [AccountProfileRead]

	init(results: [AccountProfileRead]) {
		self.results = results
	}

	func accounts(
		authority _: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[profileTestAccount()]
	}

	func inventory(
		for account: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		profileTestInventory(revision: account.accountRevision)
	}

	func profile(
		for _: ResetCardAccountRecord,
		includeEmail _: Bool
	) async throws -> AccountProfileRead {
		guard results.isEmpty == false else {
			throw ResetCardClientError.invalidResponse
		}
		return results.removeFirst()
	}

	func use(
		_: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(
		for _: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}
}

private actor AtomicEmailProfileClient: ResetCardClient, AccountProfileClient {
	enum Mode: Equatable {
		case suspendAll
		case cacheFirstAndRetrySecond
	}

	static let firstAccountID = "11111111-1111-4111-8111-111111111111"
	static let secondAccountID = "22222222-2222-4222-8222-222222222222"

	private let mode: Mode
	private var visibleContinuations = [
		String: CheckedContinuation<AccountProfileRead, Error>
	]()
	private var secondAccountRequestCount = 0

	init(mode: Mode = .suspendAll) {
		self.mode = mode
	}

	func accounts(
		authority _: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[
			Self.account(
				id: Self.firstAccountID,
				alias: "Account 00000-00001"
			),
			Self.account(
				id: Self.secondAccountID,
				alias: "Account 00000-00002"
			),
		]
	}

	func inventory(
		for account: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		ResetCardInventory(
			authority: profileTestAuthority,
			accountID: account.accountID,
			accountRevision: account.accountRevision,
			cards: [],
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)
	}

	func profile(
		for account: ResetCardAccountRecord,
		includeEmail: Bool
	) async throws -> AccountProfileRead {
		guard includeEmail else {
			return Self.profile(account: account, email: nil)
		}
		if mode == .cacheFirstAndRetrySecond {
			if account.accountID == Self.firstAccountID {
				return Self.profile(account: account, email: "first@example.com")
			}
			secondAccountRequestCount += 1
			if secondAccountRequestCount == 1 {
				throw ResetCardClientError.timedOut
			}
		}
		return try await withCheckedThrowingContinuation { continuation in
			visibleContinuations[account.accountID] = continuation
		}
	}

	func use(
		_: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(
		for _: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func waitForVisibleRequests(count: Int = 2) async throws {
		for _ in 0 ..< 2_000 {
			if visibleContinuations.count == count {
				return
			}
			await Task.yield()
		}
		throw WaitError.timedOut
	}

	func resolveVisibleProfile(accountID: String) {
		let account = Self.account(
			id: accountID,
			alias: accountID == Self.firstAccountID
				? "Account 00000-00001"
				: "Account 00000-00002"
		)
		visibleContinuations.removeValue(forKey: accountID)?.resume(
			returning: Self.profile(
				account: account,
				email: accountID == Self.firstAccountID
					? "first@example.com"
					: "second@example.com"
			)
		)
	}

	private static func account(id: String, alias: String) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: profileTestAuthority,
			accountID: id,
			alias: alias,
			accountRevision: 7,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
	}

	private static func profile(
		account: ResetCardAccountRecord,
		email: String?
	) -> AccountProfileRead {
		.available(
			AccountProfileObservation(
				accountID: account.accountID,
				accountRevision: account.accountRevision,
				observedAtUnixMicros: 1_785_276_000_000_000,
				email: email,
				planType: "pro",
				displayName: nil,
				username: nil,
				snapshot: AccountProfileSnapshot(
					lifetimeTokens: 1_000,
					peakDailyTokens: 1_000,
					longestTaskSeconds: 60,
					currentStreakDays: 1,
					longestStreakDays: 1,
					dailyUsage: []
				),
				freshness: .current
			)
		)
	}
}

private actor SuspendedProfileStoreClient: ResetCardClient, AccountProfileClient {
	private let inventoryRevision: UInt64
	private var emailRequests = [Bool]()
	private var continuations = [
		Int: CheckedContinuation<AccountProfileRead, Error>
	]()

	init(inventoryRevision: UInt64 = 7) {
		self.inventoryRevision = inventoryRevision
	}

	func accounts(
		authority _: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[profileTestAccount()]
	}

	func inventory(
		for _: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		profileTestInventory(revision: inventoryRevision)
	}

	func profile(
		for _: ResetCardAccountRecord,
		includeEmail: Bool
	) async throws -> AccountProfileRead {
		let request = emailRequests.count
		emailRequests.append(includeEmail)
		return try await withCheckedThrowingContinuation { continuation in
			continuations[request] = continuation
		}
	}

	func use(
		_: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(
		for _: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func requestCount() -> Int {
		emailRequests.count
	}

	func requestedEmailVisibility() -> [Bool] {
		emailRequests
	}

	func resolve(
		request: Int,
		with result: AccountProfileRead
	) {
		continuations.removeValue(forKey: request)?.resume(returning: result)
	}
}

private actor RevisionAdvanceAfterUnavailableClient: ResetCardClient, AccountProfileClient {
	private var inventoryContinuation: CheckedContinuation<ResetCardInventory, Error>?

	func accounts(
		authority _: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[profileTestAccount(revision: 7)]
	}

	func inventory(
		for _: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		try await withCheckedThrowingContinuation { continuation in
			inventoryContinuation = continuation
		}
	}

	func profile(
		for _: ResetCardAccountRecord,
		includeEmail _: Bool
	) async throws -> AccountProfileRead {
		.unavailable(
			AccountProfileUnavailable(
				error: .providerUnavailable,
				claims: AccountProfileClaims(email: nil, planType: "old-plan")
			)
		)
	}

	func use(
		_: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(
		for _: ResetCardUseAttempt
	) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func isInventoryWaiting() -> Bool {
		inventoryContinuation != nil
	}

	func resolveInventory() {
		inventoryContinuation?.resume(returning: profileTestInventory(revision: 8))
		inventoryContinuation = nil
	}
}

private actor PerAccountGenerationClient: AccountControlClient, AccountProfileClient {
	enum Mode {
		case blockSecond
		case failThenBlockSecond
		case blockInitialFirst
	}

	static let firstAccountID = "11111111-1111-4111-8111-111111111111"
	static let secondAccountID = "22222222-2222-4222-8222-222222222222"

	private static let authority = ResetCardAuthority(
		profileName: "local",
		serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	)

	private let mode: Mode
	private var firstAccount: ResetCardAccountRecord
	private let secondAccount: ResetCardAccountRecord
	private var firstContinuation: CheckedContinuation<Void, Never>?
	private var secondContinuation: CheckedContinuation<Void, Never>?
	private var didSuspendFirst = false
	private var secondRequestCount = 0

	init(mode: Mode = .blockSecond) {
		self.mode = mode
		firstAccount = Self.makeAccount(
			accountID: Self.firstAccountID,
			alias: "Account 00000-00001",
			revision: 7
		)
		secondAccount = Self.makeAccount(
			accountID: Self.secondAccountID,
			alias: "Account 00000-00002",
			revision: 7
		)
	}

	func accountSnapshot(
		authority _: ResetCardAuthority?
	) async throws -> AccountControlSnapshot {
		AccountControlSnapshot(
			authority: Self.authority,
			accounts: [firstAccount, secondAccount],
			routing: AccountRoutingControl(
				revision: 1,
				mode: .balanced,
				order: [firstAccount.accountID, secondAccount.accountID]
			)
		)
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		try await accountSnapshot(authority: authority).accounts
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		ResetCardInventory(
			authority: Self.authority,
			accountID: account.accountID,
			accountRevision: account.accountRevision,
			cards: [],
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)
	}

	func profile(
		for account: ResetCardAccountRecord,
		includeEmail: Bool
	) async throws -> AccountProfileRead {
		if mode == .blockInitialFirst,
			account.accountID == Self.firstAccountID,
			account.accountRevision == 7,
			didSuspendFirst == false
		{
			didSuspendFirst = true
			await withCheckedContinuation { continuation in
				firstContinuation = continuation
			}
		}
		if account.accountID == Self.secondAccountID {
			secondRequestCount += 1
			if mode == .failThenBlockSecond, secondRequestCount == 1 {
				throw ResetCardClientError.timedOut
			}
			if mode != .blockInitialFirst {
				await withCheckedContinuation { continuation in
					secondContinuation = continuation
				}
			}
		}
		let email: String? = if includeEmail {
			account.accountID == Self.firstAccountID
				? "first@example.com"
				: "second@example.com"
		} else {
			nil
		}
		let tokens: UInt64 = account.accountID == Self.firstAccountID
			? account.accountRevision * 1_000
			: 2_000
		return .available(
			AccountProfileObservation(
				accountID: account.accountID,
				accountRevision: account.accountRevision,
				observedAtUnixMicros: Int64(account.accountRevision) * 1_000,
				email: email,
				planType: "pro",
				displayName: nil,
				username: nil,
				snapshot: AccountProfileSnapshot(
					lifetimeTokens: tokens,
					peakDailyTokens: tokens,
					longestTaskSeconds: 60,
					currentStreakDays: 1,
					longestStreakDays: 1,
					dailyUsage: []
				),
				freshness: .current
			)
		)
	}

	func firstProfileIsPending() -> Bool {
		firstContinuation != nil
	}

	func secondProfileIsPending() -> Bool {
		secondContinuation != nil
	}

	func waitForFirstProfile() async throws {
		for _ in 0 ..< 2_000 {
			if firstContinuation != nil {
				return
			}
			await Task.yield()
		}
		throw WaitError.timedOut
	}

	func waitForSecondProfile() async throws {
		for _ in 0 ..< 2_000 {
			if secondContinuation != nil {
				return
			}
			await Task.yield()
		}
		throw WaitError.timedOut
	}

	func releaseFirstProfile() {
		firstContinuation?.resume()
		firstContinuation = nil
	}

	func releaseSecondProfile() {
		secondContinuation?.resume()
		secondContinuation = nil
	}

	func codexAuthProjection(
		authority _: ResetCardAuthority?
	) async throws -> CodexAuthProjection {
		.unmanaged
	}

	func use(_: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(for _: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func enrollFromSharedCodex(
		authority _: ResetCardAuthority?,
		operationID _: String,
		accountID _: String,
		enabled _: Bool,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func setAccountEnabled(
		authority _: ResetCardAuthority?,
		accountID: String,
		enabled _: Bool,
		expectedRevision: UInt64,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		guard accountID == firstAccount.accountID,
			expectedRevision == firstAccount.accountRevision
		else {
			throw AccountControlError.invalidInput
		}
		firstAccount = Self.makeAccount(
			accountID: firstAccount.accountID,
			alias: firstAccount.alias,
			revision: firstAccount.accountRevision + 1
		)
		return .accountChanged(firstAccount)
	}

	func logoutAccount(
		authority _: ResetCardAuthority?,
		operationID _: String,
		accountID _: String,
		expectedRevision _: UInt64,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func setFixedSelection(
		authority _: ResetCardAuthority?,
		accountID _: String,
		expectedAccountRevision _: UInt64,
		expectedRoutingRevision _: UInt64,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func setBalancedSelection(
		authority _: ResetCardAuthority?,
		expectedRoutingRevision _: UInt64,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func setAccountOrder(
		authority _: ResetCardAuthority?,
		order _: [String],
		expectedRoutingRevision _: UInt64,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func useAccountInCodex(
		authority _: ResetCardAuthority?,
		accountID _: String,
		expectedRevision _: UInt64,
		idempotencyKey _: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	private static func makeAccount(
		accountID: String,
		alias: String,
		revision: UInt64
	) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: authority,
			accountID: accountID,
			alias: alias,
			accountRevision: revision,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			credentialBinding: AccountCredentialBinding(
				schemaVersion: 1,
				version: revision,
				fingerprintSHA256: String(repeating: "a", count: 64),
				provider: .chatGPT,
				providerAccountID: accountID
			),
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
	}
}

private let profileTestAuthority = ResetCardAuthority(
	profileName: "local",
	serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
)

private func profileTestAccount(
	revision: UInt64 = 7,
	observedState: ResetCardObservedState = .available
) -> ResetCardAccountRecord {
	ResetCardAccountRecord(
		authority: profileTestAuthority,
		accountID: "11111111-1111-4111-8111-111111111111",
		alias: "Account 00000-00001",
		accountRevision: revision,
		enabled: true,
		observedState: observedState,
		lifecycleReadiness: .ready,
		fiveHourQuota: .unknown(durationMinutes: 300),
		sevenDayQuota: .unknown(durationMinutes: 10_080)
	)
}

private func profileTestInventory(revision: UInt64) -> ResetCardInventory {
	ResetCardInventory(
		authority: profileTestAuthority,
		accountID: profileTestAccount().accountID,
		accountRevision: revision,
		cards: [],
		fiveHourQuota: .unknown(durationMinutes: 300),
		sevenDayQuota: .unknown(durationMinutes: 10_080),
		observationError: nil
	)
}

private func profileObservation(
	observedAt: Int64,
	lifetimeTokens: UInt64,
	email: String? = nil,
	revision: UInt64 = 7,
	freshness: AccountProfileFreshness = .current
) -> AccountProfileObservation {
	AccountProfileObservation(
		accountID: profileTestAccount().accountID,
		accountRevision: revision,
		observedAtUnixMicros: observedAt,
		email: email,
		planType: "pro",
		displayName: "Iris",
		username: "iris",
		snapshot: AccountProfileSnapshot(
			lifetimeTokens: lifetimeTokens,
			peakDailyTokens: lifetimeTokens,
			longestTaskSeconds: 60,
			currentStreakDays: 1,
			longestStreakDays: 2,
			dailyUsage: [
				AccountProfileDailyUsage(
					date: "2026-07-28",
					tokens: lifetimeTokens
				),
			]
		),
		freshness: freshness
	)
}

@MainActor
private struct PendingFixture {
	let directory: URL
	let store: ResetCardPendingAttemptStore

	init() throws {
		directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		try FileManager.default.createDirectory(
			at: directory,
			withIntermediateDirectories: true
		)
		store = ResetCardPendingAttemptStore(
			journalURL: directory.appendingPathComponent("pending.json")
		)
	}

	func remove() {
		try? FileManager.default.removeItem(at: directory)
	}
}
