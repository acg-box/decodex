import Foundation
import XCTest
@testable import DecodexApp

@MainActor
final class ResetCardStoreStartupRetryTests: XCTestCase {
	func testStartupRetriesDisconnectedAndUnavailableReadsUntilInventoryLoads() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let account = Self.account
		let expectedInventory = try Self.inventory
		let client = ScriptedResetCardClient(
			accountSteps: [
				.failure(.commandFailed),
				.failure(.service(.productStateUnavailable)),
				.value([account]),
			],
			accountFallback: .value([account]),
			inventoryFallback: .value(expectedInventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [.milliseconds(0), .milliseconds(0)]
		)

		store.start()
		try await waitUntil {
			store.accounts.first?.inventory == expectedInventory
				&& store.isRefreshing == false
		}

		let counts = await client.callCounts()
		XCTAssertNil(store.message)
		XCTAssertTrue(store.hasLoaded)
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 3, inventory: 1, status: 0, use: 0)
		)
	}

	func testStartupDoesNotLoopOnRowScopedInventoryFailure() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let expectedInventory = try Self.inventory
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([Self.account]),
			inventorySteps: [
				.failure(.service(.productStateUnavailable)),
				.value(expectedInventory),
			],
			inventoryFallback: .value(expectedInventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [.milliseconds(0)]
		)

		store.start()
		try await waitUntil {
			store.hasLoaded && store.isRefreshing == false
		}
		try await Task.sleep(for: .milliseconds(20))

		let counts = await client.callCounts()
		XCTAssertNil(store.accounts.first?.inventory)
		XCTAssertEqual(
			store.accounts.first?.error,
			.service(.productStateUnavailable)
		)
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 1, inventory: 1, status: 0, use: 0)
		)
	}

	func testStartupDoesNotRetryPermanentReadFailure() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ScriptedResetCardClient(
			accountSteps: [.failure(.service(.schemaUnsupported))],
			accountFallback: .value([Self.account]),
			inventoryFallback: .value(try Self.inventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [.milliseconds(0), .milliseconds(0)]
		)

		store.start()
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts == 1 && store.hasLoaded && store.isRefreshing == false
		}
		try await Task.sleep(for: .milliseconds(20))

		let counts = await client.callCounts()
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 1, inventory: 0, status: 0, use: 0)
		)
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .error,
				text: "The selected Codex version does not support reset cards."
			)
		)
	}

	func testStartupRetryScheduleIsFinite() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .failure(.commandFailed),
			inventoryFallback: .value(try Self.inventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [
				.milliseconds(0),
				.milliseconds(0),
				.milliseconds(0),
			]
		)

		store.start()
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts == 4 && store.isRefreshing == false
		}
		try await Task.sleep(for: .milliseconds(20))

		let counts = await client.callCounts()
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 4, inventory: 0, status: 0, use: 0)
		)
	}

	func testManualRefreshPerformsOneReadWithoutStartingAutomaticRetries() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .failure(.commandFailed),
			inventoryFallback: .value(try Self.inventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [
				.milliseconds(0),
				.milliseconds(0),
				.milliseconds(0),
			]
		)

		await store.refresh()

		let counts = await client.callCounts()
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 1, inventory: 0, status: 0, use: 0)
		)
	}

	func testManualRefreshChecksPendingStatusOnceWithoutDispatchingUse() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let attempt = try Self.attempt
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([]),
			inventoryFallback: .value(try Self.inventory),
			statusSteps: [.value(.unavailable(.productStateUnavailable))],
			statusFallback: .value(.completed(.reset))
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [.milliseconds(0), .milliseconds(0)]
		)

		await store.refresh()

		let counts = await client.callCounts()
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 1, inventory: 0, status: 1, use: 0)
		)
		XCTAssertEqual(store.pendingAttempts, [attempt])
	}

	func testStartupRetriesPendingStatusReadWithoutDispatchingUse() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let attempt = try Self.attempt
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([]),
			inventoryFallback: .value(try Self.inventory),
			statusSteps: [
				.value(.unavailable(.productStateUnavailable)),
				.value(.prepared),
				.value(.effectAmbiguous),
				.value(.completed(.reset)),
			],
			statusFallback: .value(.completed(.reset))
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [
				.milliseconds(0),
				.milliseconds(0),
				.milliseconds(0),
			]
		)

		store.start()
		try await waitUntil {
			store.pendingAttempts.isEmpty
				&& fixture.store.load() == .available([])
		}

		let counts = await client.callCounts()
			XCTAssertEqual(
				counts,
				ClientCallCounts(accounts: 1, inventory: 0, status: 4, use: 0)
			)
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(tone: .success, text: "Usage restored.")
		)
	}

	func testStartupRetriesMissingPendingStatusWithoutDispatchingUse() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let attempt = try Self.attempt
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([]),
			inventoryFallback: .value(try Self.inventory),
			statusSteps: [.value(.notFound)],
			statusFallback: .value(.notFound)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [.milliseconds(0), .milliseconds(0)]
		)

		store.start()
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.status == 3 && store.isRefreshing == false
		}
		try await Task.sleep(for: .milliseconds(20))

		let counts = await client.callCounts()
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 1, inventory: 0, status: 3, use: 0)
		)
		XCTAssertEqual(store.pendingAttempts, [attempt])
	}

	func testRefreshPublishesRowsAndCompletesEachInventoryIndependently() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let firstAccount = Self.account(authority: nil)
		let secondAccount = ResetCardAccountRecord(
			authority: nil,
			accountID: "33333333-3333-4333-8333-333333333333",
			alias: "Account 00000-00002",
			accountRevision: 11,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
		let firstInventory = try Self.inventory
		let secondInventory = ResetCardInventory(
			authority: Self.authority,
			accountID: secondAccount.accountID,
			accountRevision: secondAccount.accountRevision,
			cards: [],
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)
		let client = ProgressiveResetCardClient(
			accounts: [firstAccount, secondAccount],
			inventories: [
				firstAccount.accountID: firstInventory,
				secondAccount.accountID: secondInventory,
			],
			blockedAccountID: secondAccount.accountID
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refresh = Task {
			await store.refresh()
		}
		try await waitUntil {
			store.accounts.map(\.id) == [
				firstAccount.accountID,
				secondAccount.accountID,
			]
				&& store.accounts[0].inventory == firstInventory
				&& store.accounts[0].isRefreshing == false
				&& store.accounts[1].inventory == nil
				&& store.accounts[1].isRefreshing
		}

		await client.releaseBlockedAccount()
		await refresh.value

		XCTAssertEqual(store.accounts.map(\.inventory), [firstInventory, secondInventory])
		XCTAssertTrue(store.accounts.allSatisfy { $0.isRefreshing == false })
	}

	func testRefreshPinsTheAccountListToTheEstablishedAuthority() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([Self.account(authority: nil)]),
			inventoryFallback: .value(try Self.inventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.refresh()

		let authorities = await client.accountAuthorities()
		XCTAssertEqual(authorities.count, 2)
		XCTAssertNil(authorities[0])
		XCTAssertEqual(authorities[1], Self.authority)
		XCTAssertEqual(store.accounts.map(\.account.authority), [Self.authority])
	}

	func testFirstRefreshPreservesDiscoveredAuthorityForInventoryRead() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([Self.account]),
			inventoryFallback: .value(try Self.inventory),
			requiredInventoryAuthority: Self.authority
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()

		XCTAssertEqual(store.accounts.first?.account.authority, Self.authority)
		XCTAssertEqual(store.accounts.first?.inventory, try Self.inventory)
		XCTAssertNil(store.accounts.first?.error)
	}

	func testExplicitUseDispatchesOnlyOnce() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let inventory = try Self.inventory
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([Self.account]),
			inventoryFallback: .value(inventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [.milliseconds(0), .milliseconds(0)]
		)
		await store.refresh()

		let completion = await store.use(try Self.attempt)
		try await Task.sleep(for: .milliseconds(20))

		let counts = await client.callCounts()
		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: false))
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 1, inventory: 1, status: 0, use: 1)
		)
	}

	private func makePendingFixture() throws -> StartupRetryPendingFixture {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		try FileManager.default.createDirectory(
			at: directory,
			withIntermediateDirectories: true
		)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: directory.path
		)

		return StartupRetryPendingFixture(
			directory: directory,
			store: ResetCardPendingAttemptStore(
				journalURL: directory.appendingPathComponent("pending.json")
			)
		)
	}

	private func waitUntil(
		_ predicate: @escaping @MainActor () async -> Bool
	) async throws {
		for _ in 0..<200 {
			if await predicate() {
				return
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTFail("Timed out waiting for the startup retry state.")
	}

	private static let authority = ResetCardAuthority(
		profileName: "local",
		serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	)

	private static let account = account(authority: authority)

	private static func account(
		authority: ResetCardAuthority?
	) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: authority,
			accountID: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405160",
			alias: "Account 00000-00001",
			accountRevision: 7,
			enabled: true,
			observedState: .depleted,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
	}

	private static var inventory: ResetCardInventory {
		get throws {
			ResetCardInventory(
				authority: authority,
				accountID: account.accountID,
				accountRevision: account.accountRevision,
				cards: [
					try ResetCardDescriptor(
						grantedAtUnixSeconds: 100,
						expiresAtUnixSeconds: 200
					),
				],
				fiveHourQuota: ResetCardQuotaWindow(
					durationMinutes: 300,
					observedAtUnixMicros: 1_000_000,
					state: .current(
						usedPercent: 100,
						resetsAtUnixMicros: 2_000_000
					)
				),
				sevenDayQuota: ResetCardQuotaWindow(
					durationMinutes: 10_080,
					observedAtUnixMicros: 1_000_000,
					state: .stale(
						usedPercent: 80,
						resetsAtUnixMicros: 3_000_000
					)
				),
				observationError: nil
			)
		}
	}

	private static var attempt: ResetCardUseAttempt {
		get throws {
			ResetCardUseAttempt(
				target: ResetCardUseTarget(
					authority: authority,
					accountID: account.accountID,
					expectedRevision: account.accountRevision,
					descriptor: try ResetCardDescriptor(
						grantedAtUnixSeconds: 100,
						expiresAtUnixSeconds: 200
					)
				),
				idempotencyKey: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405161"
			)
		}
	}
}

private enum ClientStep<Value: Sendable>: Sendable {
	case value(Value)
	case failure(ResetCardClientError)
}

private struct ClientCallCounts: Equatable, Sendable {
	let accounts: Int
	let inventory: Int
	let status: Int
	let use: Int
}

private actor ScriptedResetCardClient: ResetCardClient {
	private var accountSteps: [ClientStep<[ResetCardAccountRecord]>]
	private let accountFallback: ClientStep<[ResetCardAccountRecord]>
	private var inventorySteps: [ClientStep<ResetCardInventory>]
	private let inventoryFallback: ClientStep<ResetCardInventory>
	private var statusSteps: [ClientStep<ResetCardOperationState>]
	private let statusFallback: ClientStep<ResetCardOperationState>
	private let requiredInventoryAuthority: ResetCardAuthority?
	private var counts = ClientCallCounts(accounts: 0, inventory: 0, status: 0, use: 0)
	private var requestedAccountAuthorities = [ResetCardAuthority?]()

	init(
		accountSteps: [ClientStep<[ResetCardAccountRecord]>],
		accountFallback: ClientStep<[ResetCardAccountRecord]>,
		inventorySteps: [ClientStep<ResetCardInventory>] = [],
		inventoryFallback: ClientStep<ResetCardInventory>,
		statusSteps: [ClientStep<ResetCardOperationState>] = [],
		statusFallback: ClientStep<ResetCardOperationState> = .value(.notFound),
		requiredInventoryAuthority: ResetCardAuthority? = nil
	) {
		self.accountSteps = accountSteps
		self.accountFallback = accountFallback
		self.inventorySteps = inventorySteps
		self.inventoryFallback = inventoryFallback
		self.statusSteps = statusSteps
		self.statusFallback = statusFallback
		self.requiredInventoryAuthority = requiredInventoryAuthority
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		requestedAccountAuthorities.append(authority)
		counts = ClientCallCounts(
			accounts: counts.accounts + 1,
			inventory: counts.inventory,
			status: counts.status,
			use: counts.use
		)
		return try Self.resolve(
			accountSteps.isEmpty ? accountFallback : accountSteps.removeFirst()
		)
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		counts = ClientCallCounts(
			accounts: counts.accounts,
			inventory: counts.inventory + 1,
			status: counts.status,
			use: counts.use
		)
		if let requiredInventoryAuthority,
			account.authority != requiredInventoryAuthority
		{
			throw ResetCardClientError.invalidResponse
		}
		return try Self.resolve(
			inventorySteps.isEmpty ? inventoryFallback : inventorySteps.removeFirst()
		)
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		counts = ClientCallCounts(
			accounts: counts.accounts,
			inventory: counts.inventory,
			status: counts.status + 1,
			use: counts.use
		)
		return try Self.resolve(
			statusSteps.isEmpty ? statusFallback : statusSteps.removeFirst()
		)
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		counts = ClientCallCounts(
			accounts: counts.accounts,
			inventory: counts.inventory,
			status: counts.status,
			use: counts.use + 1
		)
		return .prepared
	}

	func callCounts() -> ClientCallCounts {
		counts
	}

	func accountAuthorities() -> [ResetCardAuthority?] {
		requestedAccountAuthorities
	}

	private static func resolve<Value: Sendable>(
		_ step: ClientStep<Value>
	) throws -> Value {
		switch step {
		case .value(let value):
			return value
		case .failure(let error):
			throw error
		}
	}
}

private actor ProgressiveResetCardClient: ResetCardClient {
	private let accountRecords: [ResetCardAccountRecord]
	private let inventories: [String: ResetCardInventory]
	private let blockedAccountID: String
	private var isReleased = false

	init(
		accounts: [ResetCardAccountRecord],
		inventories: [String: ResetCardInventory],
		blockedAccountID: String
	) {
		accountRecords = accounts
		self.inventories = inventories
		self.blockedAccountID = blockedAccountID
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		accountRecords
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		if account.accountID == blockedAccountID {
			while isReleased == false {
				try await Task.sleep(for: .milliseconds(1))
			}
		}
		guard let inventory = inventories[account.accountID] else {
			throw ResetCardClientError.invalidResponse
		}
		return inventory
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.notFound
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.prepared
	}

	func releaseBlockedAccount() {
		isReleased = true
	}
}

@MainActor
private struct StartupRetryPendingFixture {
	let directory: URL
	let store: ResetCardPendingAttemptStore

	func remove() {
		try? FileManager.default.removeItem(at: directory)
	}
}
