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
				.failure(.transportDisconnected),
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

	func testStartupRetriesRowScopedDaemonCacheMissUntilInventoryLoads() async throws {
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
		XCTAssertEqual(store.accounts.first?.inventory, expectedInventory)
		XCTAssertNil(store.accounts.first?.error)
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 2, inventory: 2, status: 0, use: 0)
		)
	}

	func testRevisionRefreshRetainsQuotaAcrossATransientInventoryFailure() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let oldInventory = try Self.inventory
		let updatedAccount = ResetCardAccountRecord(
			authority: Self.authority,
			accountID: Self.account.accountID,
			alias: Self.account.alias,
			accountRevision: 8,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
		let restoredQuota = ResetCardQuotaWindow(
			durationMinutes: 300,
			observedAtUnixMicros: 2_000_000,
			state: .current(
				usedPercent: 0,
				resetsAtUnixMicros: 4_000_000
			)
		)
		let restoredInventory = ResetCardInventory(
			authority: Self.authority,
			accountID: updatedAccount.accountID,
			accountRevision: updatedAccount.accountRevision,
			cards: [],
			fiveHourQuota: restoredQuota,
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)
		let client = ScriptedResetCardClient(
			accountSteps: [
				.value([Self.account]),
				.value([updatedAccount]),
				.value([updatedAccount]),
			],
			accountFallback: .value([updatedAccount]),
			inventorySteps: [
				.value(oldInventory),
				.failure(.transportBackpressured),
				.value(restoredInventory),
			],
			inventoryFallback: .value(restoredInventory)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		XCTAssertEqual(store.accounts.first?.inventory, oldInventory)

		await store.refresh()
		let interrupted = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(interrupted.account.accountRevision, 8)
		XCTAssertEqual(interrupted.inventory, oldInventory)
		XCTAssertEqual(interrupted.fiveHourQuota, oldInventory.fiveHourQuota)
		XCTAssertEqual(interrupted.error, .transportBackpressured)
		XCTAssertTrue(interrupted.targets.isEmpty)

		await store.refresh()
		let restored = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(restored.inventory, restoredInventory)
		XCTAssertEqual(restored.fiveHourQuota, restoredQuota)
		XCTAssertNil(restored.error)
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
			accountFallback: .failure(.transportDisconnected),
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
			accountFallback: .failure(.transportDisconnected),
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

	func testDaemonObservationSignalRefreshesUntilApplicationTermination() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ObservationDrivenResetCardClient(
			account: Self.account,
			inventory: try Self.inventory
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		store.start()
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts == 1 && counts.inventory == 1
		}
		await client.publish(generation: 1)
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts >= 2 && counts.inventory >= 2 && store.isRefreshing == false
		}

		await store.prepareForApplicationTermination()
		let stoppedCounts = await client.callCounts()
		await client.publish(generation: 2)
		try await Task.sleep(for: .milliseconds(20))
		let finalCounts = await client.callCounts()
		XCTAssertEqual(finalCounts, stoppedCounts)
	}

	func testDaemonHeartbeatDoesNotRequestAnotherRefreshForTheSameGeneration() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ObservationDrivenResetCardClient(
			account: Self.account,
			inventory: try Self.inventory
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		store.start()
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts == 1
				&& counts.inventory == 1
				&& store.isRefreshing == false
		}

		await client.publish(generation: 1)
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts == 2
				&& counts.inventory == 2
				&& store.isRefreshing == false
		}
		try await waitUntil { await client.observationIsPending() }
		let synchronizedCounts = await client.callCounts()

		// The daemon uses a same-generation heartbeat to bound a missed wake. It
		// must not turn an unchanged heartbeat into another full UI refresh.
		await client.publish(generation: 1)
		try await waitUntil { await client.observationIsPending() }
		try await Task.sleep(for: .milliseconds(50))

		let finalCounts = await client.callCounts()
		XCTAssertEqual(finalCounts, synchronizedCounts)
	}

	func testBackgroundObservationKeepsThePublishedStateUsableWhileReading() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ObservationDrivenResetCardClient(
			account: Self.account,
			inventory: try Self.inventory
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		store.start()
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts == 1
				&& counts.inventory == 1
				&& store.isRefreshing == false
		}
		await client.blockInventoryReads()
		await client.publish(generation: 1)
		try await waitUntil { await client.isInventoryReadBlocked() }

		XCTAssertFalse(store.isRefreshing)
		XCTAssertTrue(store.canPerformDirectAccountControl)
		XCTAssertFalse(store.accounts.first?.isRefreshing ?? true)

		await client.releaseInventoryRead()
		try await waitUntil {
			let counts = await client.callCounts()
			return store.isRefreshing == false && counts.inventory == 2
		}
	}

	func testPanelPriorityObservationKeepsThePublishedStateUsableWhileReading() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = ObservationDrivenResetCardClient(
			account: Self.account,
			inventory: try Self.inventory
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		store.start()
		try await waitUntil {
			let counts = await client.callCounts()
			return counts.accounts == 1
				&& counts.inventory == 1
				&& store.isRefreshing == false
		}

		await client.blockInventoryReads()
		store.ensureFresh()
		try await waitUntil {
			let priorityRefreshRequestCount = await client.priorityRefreshRequestCount()
			let inventoryReadBlocked = await client.isInventoryReadBlocked()
			return priorityRefreshRequestCount == 1 && inventoryReadBlocked
		}

		XCTAssertFalse(store.isRefreshing)
		XCTAssertTrue(store.canPerformDirectAccountControl)
		XCTAssertFalse(store.accounts.first?.isRefreshing ?? true)

		store.ensureFresh()
		try await Task.sleep(for: .milliseconds(50))
		let priorityRefreshRequestCount = await client.priorityRefreshRequestCount()
		XCTAssertEqual(priorityRefreshRequestCount, 1)

		await client.releaseInventoryRead()
		try await waitUntil {
			let counts = await client.callCounts()
			return store.isRefreshing == false && counts.inventory == 2
		}
	}

	func testTerminationRejectsRefreshWhileDrainingTheActiveCycle() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let client = TerminationBlockingResetCardClient(
			account: Self.account,
			inventory: try Self.inventory
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		store.requestRefresh()
		try await waitUntil {
			await client.isAccountReadBlocked()
		}

		let termination = Task {
			await store.prepareForApplicationTermination()
		}
		try await waitUntil {
			store.isPreparingForTermination
		}

		store.requestRefresh()
		await client.releaseAccountRead()
		await termination.value

		let accountCallCount = await client.accountCallCount()
		XCTAssertEqual(accountCallCount, 1)
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

	func testRefreshStartsEveryDaemonValueReadWithoutAThreeAccountCap() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let aliases = ["Alex", "Avery", "Bailey", "Blake", "Casey", "Clara"]
		let accounts = aliases.enumerated().map { offset, alias in
			ResetCardAccountRecord(
				authority: nil,
				accountID: String(
					format: "00000000-0000-4000-8000-%012d",
					offset + 1
				),
				alias: alias,
				accountRevision: UInt64(offset + 1),
				enabled: true,
				observedState: .available,
				lifecycleReadiness: .ready,
				fiveHourQuota: .unknown(durationMinutes: 300),
				sevenDayQuota: .unknown(durationMinutes: 10_080)
			)
		}
		let inventories = Dictionary(
			uniqueKeysWithValues: accounts.map { account in
				(
					account.accountID,
					ResetCardInventory(
						authority: Self.authority,
						accountID: account.accountID,
						accountRevision: account.accountRevision,
						cards: [],
						fiveHourQuota: account.fiveHourQuota,
						sevenDayQuota: account.sevenDayQuota,
						observationError: nil
					)
				)
			}
		)
		let client = ConcurrentDaemonValueClient(
			accounts: accounts,
			inventories: inventories
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
			await client.startedCount() == accounts.count
		}
		let startedCount = await client.startedCount()
		XCTAssertEqual(startedCount, 6)

		await client.releaseAll()
		await refresh.value
		XCTAssertTrue(store.accounts.allSatisfy { $0.inventory != nil })
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

	func testObservationFailureRetainsTheLastCompleteInventory() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let retainedInventory = try Self.inventory
		let failedObservation = ResetCardInventory(
			authority: Self.authority,
			accountID: Self.account.accountID,
			accountRevision: Self.account.accountRevision,
			cards: [],
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: .providerUnavailable
		)
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([Self.account]),
			inventorySteps: [
				.value(retainedInventory),
				.value(failedObservation),
			],
			inventoryFallback: .value(failedObservation)
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.refresh()

		let state = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(state.inventory, retainedInventory)
		XCTAssertEqual(state.fiveHourQuota, retainedInventory.fiveHourQuota)
		XCTAssertEqual(state.error, .service(.providerUnavailable))
		XCTAssertTrue(state.targets.isEmpty)
		XCTAssertEqual(
			ResetCardInventoryPresentation(
				state: state,
				isAwaitingFreshAccountSkeleton: false
			),
			.updating(detail: ResetCardServiceError.providerUnavailable.presentation)
		)
	}

	func testCompletedUseReconcilesInBackgroundAcrossTransientContention() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let retainedInventory = try Self.inventory
		let restoredQuota = ResetCardQuotaWindow(
			durationMinutes: 300,
			observedAtUnixMicros: 2_000_000,
			state: .current(
				usedPercent: 0,
				resetsAtUnixMicros: 4_000_000
			)
		)
		let restoredInventory = ResetCardInventory(
			authority: Self.authority,
			accountID: Self.account.accountID,
			accountRevision: Self.account.accountRevision,
			cards: [],
			fiveHourQuota: restoredQuota,
			sevenDayQuota: retainedInventory.sevenDayQuota,
			observationError: nil
		)
		let contendedObservation = ResetCardInventory(
			authority: Self.authority,
			accountID: Self.account.accountID,
			accountRevision: Self.account.accountRevision,
			cards: [],
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: .resourceExhausted
		)
		let client = ScriptedResetCardClient(
			accountSteps: [],
			accountFallback: .value([Self.account]),
			inventorySteps: [
				.value(retainedInventory),
				.value(contendedObservation),
				.value(restoredInventory),
			],
			inventoryFallback: .value(restoredInventory),
			useFallback: .value(.completed(.reset))
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			postUseRetryDelays: [.milliseconds(100)]
		)
		await store.refresh()

		let attempt = try Self.attempt
		let completion = await store.use(attempt)
		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: true))
		try await waitUntil {
			store.accounts.first?.error == .service(.resourceExhausted)
		}

		let updating = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(updating.inventory, retainedInventory)
		XCTAssertEqual(updating.fiveHourQuota, retainedInventory.fiveHourQuota)
		XCTAssertTrue(updating.isRefreshing)
		XCTAssertTrue(store.blocksNewAttempt(for: attempt.target))
		XCTAssertEqual(
			ResetCardInventoryPresentation(
				state: updating,
				isAwaitingFreshAccountSkeleton: false
			),
			.updating(detail: ResetCardServiceError.resourceExhausted.presentation)
		)

		try await waitUntil {
			store.accounts.first?.inventory == restoredInventory
				&& store.accounts.first?.isRefreshing == false
		}
		let restored = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(restored.fiveHourQuota, restoredQuota)
		XCTAssertNil(restored.error)
		let counts = await client.callCounts()
		XCTAssertEqual(
			counts,
			ClientCallCounts(accounts: 1, inventory: 3, status: 0, use: 1)
		)
	}

	func testUseWaitsForPreEffectReadAndReconciliationRemainsAuthoritative() async throws {
		let fixture = try makePendingFixture()
		defer { fixture.remove() }
		let retainedInventory = try Self.inventory
		let restoredInventory = ResetCardInventory(
			authority: Self.authority,
			accountID: Self.account.accountID,
			accountRevision: Self.account.accountRevision,
			cards: [],
			fiveHourQuota: ResetCardQuotaWindow(
				durationMinutes: 300,
				observedAtUnixMicros: 2_000_000,
				state: .current(
					usedPercent: 0,
					resetsAtUnixMicros: 4_000_000
				)
			),
			sevenDayQuota: retainedInventory.sevenDayQuota,
			observationError: nil
		)
		let client = OverlappingPostUseClient(
			account: Self.account,
			retainedInventory: retainedInventory,
			restoredInventory: restoredInventory,
			preEffectFailure: .invalidResponse
		)
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			postUseRetryDelays: []
		)
		await store.refresh()

		let preEffectRefresh = Task {
			await store.refresh()
		}
		try await waitUntil {
			await client.isInventoryCallPending(2)
		}

		let attempt = try Self.attempt
		let useTask = Task {
			await store.use(attempt)
		}
		try await Task.sleep(for: .milliseconds(20))
		let useCallsBeforeReadRelease = await client.useCallCount()
		XCTAssertEqual(
			useCallsBeforeReadRelease,
			0,
			"Use must wait for the older daemon-value read."
		)

		await client.releaseInventoryCall(2)
		let completion = await useTask.value
		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: true))
		try await waitUntil {
			await client.isInventoryCallPending(3)
		}
		await preEffectRefresh.value

		let afterPreEffectRead = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(afterPreEffectRead.inventory, retainedInventory)
		XCTAssertNil(afterPreEffectRead.error)
		XCTAssertTrue(afterPreEffectRead.isRefreshing)
		XCTAssertTrue(store.blocksNewAttempt(for: attempt.target))
		XCTAssertEqual(
			ResetCardInventoryPresentation(
				state: afterPreEffectRead,
				isAwaitingFreshAccountSkeleton: false
			),
			.updating(detail: nil)
		)

		await client.releaseInventoryCall(3)
		try await waitUntil {
			store.accounts.first?.inventory == restoredInventory
				&& store.accounts.first?.isRefreshing == false
		}
		let maximumActiveInventoryCalls = await client.maximumActiveInventoryCalls()
		let useOverlappedInventory = await client.didUseOverlapInventory()
		XCTAssertEqual(maximumActiveInventoryCalls, 1)
		XCTAssertFalse(useOverlappedInventory)
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
					state: .current(
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

private actor ObservationDrivenResetCardClient: ResetCardClient, AccountObservationClient {
	private let account: ResetCardAccountRecord
	private let inventoryValue: ResetCardInventory
	private var accountCalls = 0
	private var inventoryCalls = 0
	private var inventoryReadsBlocked = false
	private var inventoryReadContinuation: CheckedContinuation<Void, Never>?
	private var generations = [UInt64]()
	private var signalContinuation: CheckedContinuation<AccountObservationSignal, Never>?
	private var priorityRefreshRequests = 0

	init(account: ResetCardAccountRecord, inventory: ResetCardInventory) {
		self.account = account
		inventoryValue = inventory
	}

	func accounts(authority _: ResetCardAuthority?) async throws -> [ResetCardAccountRecord] {
		accountCalls += 1
		return [account]
	}

	func inventory(for _: ResetCardAccountRecord) async throws -> ResetCardInventory {
		inventoryCalls += 1
		if inventoryReadsBlocked {
			await withCheckedContinuation { continuation in
				inventoryReadContinuation = continuation
			}
		}
		return inventoryValue
	}

	func blockInventoryReads() {
		inventoryReadsBlocked = true
	}

	func isInventoryReadBlocked() -> Bool {
		inventoryReadContinuation != nil
	}

	func releaseInventoryRead() {
		inventoryReadsBlocked = false
		let continuation = inventoryReadContinuation
		inventoryReadContinuation = nil
		continuation?.resume()
	}

	func waitForAccountObservation(
		afterGeneration _: UInt64
	) async throws -> AccountObservationSignal {
		if generations.isEmpty == false {
			return AccountObservationSignal(generation: generations.removeFirst())
		}
		return await withCheckedContinuation { continuation in
			signalContinuation = continuation
		}
	}

	func requestAccountObservationRefresh(
		afterGeneration _: UInt64
	) async throws -> AccountObservationSignal {
		priorityRefreshRequests += 1
		return AccountObservationSignal(generation: 0)
	}

	func priorityRefreshRequestCount() -> Int {
		priorityRefreshRequests
	}

	func publish(generation: UInt64) {
		if let signalContinuation {
			self.signalContinuation = nil
			signalContinuation.resume(returning: AccountObservationSignal(generation: generation))
		} else {
			generations.append(generation)
		}
	}

	func callCounts() -> ClientCallCounts {
		ClientCallCounts(accounts: accountCalls, inventory: inventoryCalls, status: 0, use: 0)
	}

	func observationIsPending() -> Bool {
		signalContinuation != nil
	}

	func use(_: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
	}

	func status(for _: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		throw ResetCardClientError.invalidResponse
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
	private var useSteps: [ClientStep<ResetCardOperationState>]
	private let useFallback: ClientStep<ResetCardOperationState>
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
		useSteps: [ClientStep<ResetCardOperationState>] = [],
		useFallback: ClientStep<ResetCardOperationState> = .value(.prepared),
		requiredInventoryAuthority: ResetCardAuthority? = nil
	) {
		self.accountSteps = accountSteps
		self.accountFallback = accountFallback
		self.inventorySteps = inventorySteps
		self.inventoryFallback = inventoryFallback
		self.statusSteps = statusSteps
		self.statusFallback = statusFallback
		self.useSteps = useSteps
		self.useFallback = useFallback
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
		return try Self.resolve(
			useSteps.isEmpty ? useFallback : useSteps.removeFirst()
		)
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

private actor OverlappingPostUseClient: ResetCardClient {
	private let account: ResetCardAccountRecord
	private let retainedInventory: ResetCardInventory
	private let restoredInventory: ResetCardInventory
	private let preEffectFailure: ResetCardClientError?
	private var inventoryCalls = 0
	private var activeInventoryCalls = 0
	private var maximumActiveCalls = 0
	private var useCalls = 0
	private var useOverlappedInventory = false
	private var pendingInventoryCalls = [
		Int: CheckedContinuation<Void, Never>
	]()

	init(
		account: ResetCardAccountRecord,
		retainedInventory: ResetCardInventory,
		restoredInventory: ResetCardInventory,
		preEffectFailure: ResetCardClientError? = nil
	) {
		self.account = account
		self.retainedInventory = retainedInventory
		self.restoredInventory = restoredInventory
		self.preEffectFailure = preEffectFailure
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		[account]
	}

	func inventory(
		for account: ResetCardAccountRecord
	) async throws -> ResetCardInventory {
		inventoryCalls += 1
		let call = inventoryCalls
		activeInventoryCalls += 1
		maximumActiveCalls = max(maximumActiveCalls, activeInventoryCalls)
		if call > 1 {
			await withCheckedContinuation { continuation in
				pendingInventoryCalls[call] = continuation
			}
		}
		activeInventoryCalls -= 1
		if call == 2, let preEffectFailure {
			throw preEffectFailure
		}
		return call < 3 ? retainedInventory : restoredInventory
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		useCalls += 1
		useOverlappedInventory = activeInventoryCalls > 0
		return .completed(.reset)
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.notFound
	}

	func isInventoryCallPending(_ call: Int) -> Bool {
		pendingInventoryCalls[call] != nil
	}

	func releaseInventoryCall(_ call: Int) {
		pendingInventoryCalls.removeValue(forKey: call)?.resume()
	}

	func maximumActiveInventoryCalls() -> Int {
		maximumActiveCalls
	}

	func useCallCount() -> Int {
		useCalls
	}

	func didUseOverlapInventory() -> Bool {
		useOverlappedInventory
	}
}

private actor TerminationBlockingResetCardClient: ResetCardClient {
	private let account: ResetCardAccountRecord
	private let retainedInventory: ResetCardInventory
	private var accountCalls = 0
	private var blockedAccountRead: CheckedContinuation<Void, Never>?

	init(account: ResetCardAccountRecord, inventory: ResetCardInventory) {
		self.account = account
		retainedInventory = inventory
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		accountCalls += 1
		await withCheckedContinuation { continuation in
			blockedAccountRead = continuation
		}
		return [account]
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		retainedInventory
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.notFound
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.prepared
	}

	func isAccountReadBlocked() -> Bool {
		blockedAccountRead != nil
	}

	func releaseAccountRead() {
		let continuation = blockedAccountRead
		blockedAccountRead = nil
		continuation?.resume()
	}

	func accountCallCount() -> Int {
		accountCalls
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

private actor ConcurrentDaemonValueClient: ResetCardClient {
	private let accountRecords: [ResetCardAccountRecord]
	private let inventories: [String: ResetCardInventory]
	private var startedAccountIDs = Set<String>()
	private var isReleased = false

	init(
		accounts: [ResetCardAccountRecord],
		inventories: [String: ResetCardInventory]
	) {
		accountRecords = accounts
		self.inventories = inventories
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		accountRecords
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		startedAccountIDs.insert(account.accountID)
		while isReleased == false {
			try await Task.sleep(for: .milliseconds(1))
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

	func startedCount() -> Int {
		startedAccountIDs.count
	}

	func releaseAll() {
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
