@testable import DecodexApp
import Foundation
import XCTest

@MainActor
final class AccountProfileStoreTests: XCTestCase {
	func testProfilesLoadIndependentlyAndEmailVisibilityScrubsRetainedState() async throws {
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
		XCTAssertEqual(emailRequests, [false, true])
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
		XCTAssertNil(store.accounts.first?.error)
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
		XCTAssertEqual(store.accounts.first?.isProfileDegraded, true)
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
		XCTAssertEqual(emailVisibility, [false, true, true])
	}

	func testOldUnavailableResultCannotAttachAfterInventoryAdvancesRevision() async throws {
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
			if store.accounts.first?.account.accountRevision == 8 {
				break
			}
			await Task.yield()
		}
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
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

	func testInventoryRevisionAdvanceClearsUnavailableResultThatArrivedFirst() async throws {
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
		XCTAssertNil(store.accounts.first?.profile)
		XCTAssertNil(store.accounts.first?.profileUnavailable)
		XCTAssertNil(store.accounts.first?.profileError)
		XCTAssertEqual(store.accounts.first?.isProfileRefreshing, false)
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
		displayLabel: "Iris",
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

private let profileTestAuthority = ResetCardAuthority(
	profileName: "local",
	serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
)

private func profileTestAccount(revision: UInt64 = 7) -> ResetCardAccountRecord {
	ResetCardAccountRecord(
		authority: profileTestAuthority,
		accountID: "11111111-1111-4111-8111-111111111111",
		displayLabel: "Iris",
		accountRevision: revision,
		enabled: true,
		observedState: .available,
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
	revision: UInt64 = 7
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
		freshness: .current
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
