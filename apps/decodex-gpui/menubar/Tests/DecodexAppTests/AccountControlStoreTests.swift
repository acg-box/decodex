@testable import DecodexApp
import Foundation
import XCTest

@MainActor
final class AccountControlStoreTests: XCTestCase {
	private let accountID = "11111111-1111-4111-8111-111111111111"
	private let authority = ResetCardAuthority(
		profileName: "local",
		serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	)

	func testStoreRoutesWithBothAccountAndRoutingRevisions() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()

		XCTAssertEqual(
			store.routing,
			AccountRoutingControl(
				revision: 9,
				mode: .balanced,
				order: [accountID]
			)
		)
		XCTAssertEqual(store.accounts.first?.account.credentialBinding?.version, 3)

		store.message = ResetCardStoreMessage(tone: .error, text: "Old unrelated error")
		await store.routeAccount(accountID)

		let recordedRequest = await client.routeRequest()
		let routeRequest = try XCTUnwrap(recordedRequest)
		XCTAssertEqual(
			routeRequest.expectedAccountRevision,
			7
		)
		XCTAssertEqual(routeRequest.expectedRoutingRevision, 9)
		XCTAssertEqual(
			store.routing,
			AccountRoutingControl(
				revision: 10,
				mode: .fixed(accountID: accountID),
				order: [accountID]
			)
		)
		XCTAssertNil(store.message)
	}

	func testLegacyPendingSnapshotDoesNotBlockOrdinaryRouteSelection() async throws {
		let secondID = "22222222-2222-4222-8222-222222222222"
		let first = accountRecord()
		let second = accountRecord(accountID: secondID, alias: "Second")
		let client = AccountControlStoreClient(
			account: first,
			secondaryAccount: second,
			authority: authority,
			pendingRoute: AccountRoutePending(
				operationID: "33333333-3333-4333-8333-333333333333",
				accountID: accountID,
				routingRevision: 9
			)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		await store.refresh()

		XCTAssertTrue(store.canRouteAccount(accountID))
		XCTAssertTrue(store.canRouteAccount(secondID))
		await store.routeAccount(secondID)
		let routeRequest = await client.routeRequest()
		XCTAssertEqual(routeRequest?.accountID, secondID)
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: secondID))
		XCTAssertNil(store.pendingRoute)
	}

	func testCurrentRouteRemainsNoOpWhenLegacyPendingSnapshotIsPresent() async throws {
		let secondID = "22222222-2222-4222-8222-222222222222"
		let first = accountRecord()
		let second = accountRecord(accountID: secondID, alias: "Second")
		let client = AccountControlStoreClient(
			account: first,
			secondaryAccount: second,
			authority: authority,
			routing: AccountRoutingControl(
				revision: 9,
				mode: .fixed(accountID: accountID),
				order: [accountID, secondID]
			),
			pendingRoute: AccountRoutePending(
				operationID: "33333333-3333-4333-8333-333333333333",
				accountID: secondID,
				routingRevision: 9
			),
			projection: .current(
				accountID: accountID,
				accountRevision: first.accountRevision,
				projectionDigest: String(repeating: "c", count: 64)
			)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		await store.refresh()

		XCTAssertFalse(store.canRouteAccount(accountID))
		await store.routeAccount(accountID)
		let routeRequest = await client.routeRequest()
		XCTAssertNil(routeRequest)
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
		XCTAssertNotNil(store.pendingRoute)
	}

	func testDirectActionsWaitForTheCurrentSkeletonBeforeDispatch() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsSnapshotAfterFirstRead: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		XCTAssertTrue(store.canPerformDirectAccountControl)

		let refreshTask = Task { await store.refresh() }
		var snapshotIsPending = false
		for _ in 0 ..< 200 {
			if await client.snapshotIsPending() {
				snapshotIsPending = true
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard snapshotIsPending else {
			await client.releaseSnapshot()
			await refreshTask.value
			return XCTFail("Snapshot read did not enter the pending state.")
		}

		XCTAssertTrue(store.isRefreshing)
		XCTAssertFalse(store.refreshSkeletonIsPublished)
		XCTAssertFalse(store.canPerformDirectAccountControl)

		await store.setAccount(accountID, enabled: false)
		await store.routeAccount(accountID)

		let blockedEnabledRequest = await client.enabledRequest()
		let blockedRouteRequest = await client.routeRequest()
		XCTAssertNil(blockedEnabledRequest)
		XCTAssertNil(blockedRouteRequest)
		XCTAssertEqual(store.routing?.mode, .balanced)
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 7)

		await client.releaseSnapshot()
		await refreshTask.value

		XCTAssertTrue(store.canPerformDirectAccountControl)
		await store.routeAccount(accountID)
		let completedRouteRequest = await client.routeRequest()
		XCTAssertNotNil(completedRouteRequest)
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
	}

	func testRefreshDoesNotStartWhileRoutingControlIsInProgress() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsRoute: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		await store.refresh()

		let routingTask = Task {
			await store.routeAccount(accountID)
		}
		var routingIsPending = false
		for _ in 0 ..< 200 {
			if await client.routeIsPending() {
				routingIsPending = true
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard routingIsPending else {
			await client.releaseRoute()
			await routingTask.value
			return XCTFail("Routing control did not enter the pending state.")
		}

		XCTAssertTrue(store.isAccountControlInProgress)
		await store.refresh()
		let readsWhileControlIsPending = await client.readCounts()
		XCTAssertEqual(readsWhileControlIsPending.snapshot, 1)

		await client.releaseRoute()
		await routingTask.value

		XCTAssertFalse(store.isAccountControlInProgress)
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
		let finalReads = await client.readCounts()
		XCTAssertEqual(finalReads.snapshot, 1)
	}

	func testObservationRefreshIsQueuedWhileRoutingControlIsInProgress() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsRoute: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		func stopObservation() async {
			await store.prepareForApplicationTermination()
			await client.publishObservation(generation: 2)
		}
		store.start()

		for _ in 0 ..< 200 {
			if store.hasLoaded,
				store.isRefreshing == false,
				store.accounts.first?.inventoryIsCurrent == true
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertTrue(store.hasLoaded)
		XCTAssertFalse(store.isRefreshing)
		XCTAssertTrue(store.accounts.first?.inventoryIsCurrent == true)

		let routingTask = Task {
			await store.routeAccount(accountID)
		}
		for _ in 0 ..< 200 {
			if await client.routeIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.routeIsPending() else {
			await client.releaseRoute()
			await routingTask.value
			await stopObservation()
			return XCTFail("Routing control did not enter the pending state.")
		}

		await client.replaceAccount(accountRecord(revision: 8))
		await client.setInventoryRevision(8, accountID: accountID)
		for _ in 0 ..< 200 {
			if await client.observationIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.observationIsPending() else {
			await client.releaseRoute()
			await routingTask.value
			await stopObservation()
			return XCTFail("Account observation signal did not enter the pending state.")
		}
		await client.publishObservation(generation: 1)
		for _ in 0 ..< 200 {
			if await client.deliveredObservationGeneration() == 1 {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		await client.releaseRoute()
		await routingTask.value

		for _ in 0 ..< 200 {
			if store.accounts.first?.account.accountRevision == 8,
				store.accounts.first?.inventory?.accountRevision == 8,
				store.accounts.first?.isRefreshing == false
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let finalState = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(finalState.account.accountRevision, 8)
		XCTAssertEqual(finalState.inventory?.accountRevision, 8)
		XCTAssertFalse(finalState.isRefreshing)
		let readsAfterObservation = await client.readCounts()
		XCTAssertGreaterThanOrEqual(readsAfterObservation.snapshot, 2)
		XCTAssertGreaterThanOrEqual(readsAfterObservation.inventory, 2)
		await stopObservation()
	}

	func testBackgroundObservationCannotOverwriteRoutingChangedDuringRead() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsSnapshotAfterFirstRead: true,
			capturesSnapshotBeforeWait: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		func stopObservation() async {
			await store.prepareForApplicationTermination()
			await client.publishObservation(generation: 2)
		}
		store.start()

		for _ in 0 ..< 200 {
			if store.hasLoaded,
				store.isRefreshing == false,
				store.accounts.first?.inventoryIsCurrent == true
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertTrue(store.hasLoaded)
		XCTAssertTrue(store.accounts.first?.inventoryIsCurrent == true)

		for _ in 0 ..< 200 {
			if await client.observationIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.observationIsPending() else {
			await stopObservation()
			return XCTFail("Account observation signal did not enter the pending state.")
		}
		await client.publishObservation(generation: 1)

		for _ in 0 ..< 200 {
			if await client.snapshotIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.snapshotIsPending() else {
			await client.releaseSnapshot()
			await stopObservation()
			return XCTFail("Background account snapshot did not enter the pending state.")
		}

		await store.routeAccount(accountID)
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
		store.message = ResetCardStoreMessage(tone: .error, text: "Keep this message")

		await client.releaseSnapshot()
		for _ in 0 ..< 200 {
			if store.isRefreshing == false,
				(await client.readCounts()).snapshot >= 2
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertEqual(
			store.routing?.mode,
			.fixed(accountID: accountID),
			"A stale background snapshot must not flash the route back to balanced."
		)
		XCTAssertEqual(store.message?.text, "Keep this message")
		await stopObservation()
	}

	func testObservationRefreshIsQueuedUntilEnrollmentCompletes() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsEnrollment: true,
			allowsEnrollment: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		func stopObservation() async {
			await store.prepareForApplicationTermination()
			await client.publishObservation(generation: 2)
		}
		store.start()

		for _ in 0 ..< 200 {
			if store.hasLoaded,
				store.isRefreshing == false,
				store.accounts.first?.inventoryIsCurrent == true
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let enrollmentTask = Task {
			await store.enrollFromSharedCodex()
		}
		for _ in 0 ..< 200 {
			if await client.enrollmentIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.enrollmentIsPending() else {
			await client.releaseEnrollment()
			await enrollmentTask.value
			await stopObservation()
			return XCTFail("Enrollment did not enter the pending state.")
		}

		await client.replaceAccount(accountRecord(revision: 8))
		await client.setInventoryRevision(8, accountID: accountID)
		for _ in 0 ..< 200 {
			if await client.observationIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.observationIsPending() else {
			await client.releaseEnrollment()
			await enrollmentTask.value
			await stopObservation()
			return XCTFail("Account observation signal did not enter the pending state.")
		}
		await client.publishObservation(generation: 1)
		await client.releaseEnrollment()
		await enrollmentTask.value

		for _ in 0 ..< 200 {
			if store.accounts.first?.account.accountRevision == 8,
				store.accounts.first?.inventory?.accountRevision == 8,
				store.accounts.first?.isRefreshing == false
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertEqual(store.accounts.first?.inventory?.accountRevision, 8)
		XCTAssertFalse(store.accounts.first?.isRefreshing == true)
		await stopObservation()
	}

	func testAdvancedInventoryRemainsVisibleWhileSkeletonReconcilesInBackground() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsSnapshotAfterFirstRead: true,
			inventoryRevisionOverride: 8
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		var reconciliationIsPending = false
		for _ in 0 ..< 200 {
			if await client.snapshotIsPending() {
				reconciliationIsPending = true
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard reconciliationIsPending else {
			await client.releaseSnapshot()
			return XCTFail("Fresh skeleton read did not enter the pending state.")
		}

		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertEqual(store.accounts.first?.account.alias, "Account 00000-00001")
		XCTAssertTrue(store.accounts.first?.account.enabled == true)
		XCTAssertEqual(store.accounts.first?.inventory?.accountRevision, 8)
		XCTAssertTrue(store.isAwaitingFreshAccountSkeleton(accountID))
		XCTAssertFalse(store.accounts.first?.isRefreshing == true)
		XCTAssertEqual(store.accounts.first?.routeCapability, .ready)

		await store.routeAccount(accountID)
		let routeRequest = await client.routeRequest()
		XCTAssertEqual(routeRequest?.expectedAccountRevision, 8)

		let refreshedAccount = accountRecord(
			alias: "Account 00000-00008",
			revision: 8,
			enabled: false,
			observedState: .authFailed,
			lifecycleReadiness: .credentialAbsent
		)
		await client.replaceAccount(refreshedAccount)
		await client.releaseSnapshot()
		for _ in 0 ..< 200 {
			if store.accounts.first?.account.accountRevision == 8,
				store.accounts.first?.inventory?.accountRevision == 8,
				store.isAwaitingFreshAccountSkeleton(accountID) == false
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertFalse(store.isAwaitingFreshAccountSkeleton(accountID))
		XCTAssertEqual(store.accounts.first?.account.alias, refreshedAccount.alias)
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertEqual(store.accounts.first?.account.enabled, false)
		XCTAssertEqual(store.accounts.first?.account.observedState, .authFailed)
		XCTAssertEqual(
			store.accounts.first?.account.lifecycleReadiness,
			.credentialAbsent
		)
		XCTAssertEqual(store.accounts.first?.inventory?.accountRevision, 8)
	}

	func testAdvancedInventoryIsReusedAfterItsSkeletonArrives() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			maxSuccessfulInventoryReads: 2
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let attempt = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: authority,
				accountID: accountID,
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			),
			idempotencyKey: "cccccccc-cccc-4ccc-8ccc-cccccccccccc"
		)
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		XCTAssertEqual(store.accounts.first?.inventory?.accountRevision, 7)

		await client.replaceAccount(accountRecord(revision: 8))
		await client.setInventoryRevision(8, accountID: accountID)
		await client.setResetCardStatus(.completed(.reset))
		await store.checkPendingStatus(attempt)

		for _ in 0 ..< 200 {
			if store.accounts.first?.account.accountRevision == 8,
				store.accounts.first?.inventory?.accountRevision == 8,
				store.accounts.first?.isRefreshing == false
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let finalState = try XCTUnwrap(store.accounts.first)
		XCTAssertEqual(finalState.account.accountRevision, 8)
		XCTAssertEqual(finalState.inventory?.accountRevision, 8)
		XCTAssertNil(finalState.error)
		XCTAssertFalse(finalState.isRefreshing)
		let reads = await client.readCounts()
		XCTAssertEqual(
			reads.inventory,
			2,
			"The advanced authoritative inventory must replace a duplicate daemon read."
		)
	}

	func testAdvancedInventoryQueuesAnotherSkeletonReadWhenOneIsInFlight() async throws {
		let secondAccountID = "22222222-2222-4222-8222-222222222222"
		let firstAccount = accountRecord()
		let secondAccount = accountRecord(
			accountID: secondAccountID,
			alias: "Account 00000-00002"
		)
		let client = AccountControlStoreClient(
			account: firstAccount,
			secondaryAccount: secondAccount,
			authority: authority,
			suspendsSnapshotAfterFirstRead: true,
			capturesSnapshotBeforeWait: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let attempt = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: authority,
				accountID: secondAccountID,
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			),
			idempotencyKey: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
		)
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.logoutAccount(accountID)
		for _ in 0 ..< 200 {
			if await client.snapshotIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.snapshotIsPending() else {
			await client.releaseSnapshot()
			return XCTFail("Skeleton reconciliation did not enter the pending state.")
		}

		await client.replaceSecondaryAccount(
			accountRecord(
				accountID: secondAccountID,
				alias: "Account 00000-00002",
				revision: 8
			)
		)
		await client.setInventoryRevision(8, accountID: secondAccountID)
		await client.setResetCardStatus(.completed(.reset))
		await store.checkPendingStatus(attempt)
		for _ in 0 ..< 200 {
			if store.isAwaitingFreshAccountSkeleton(secondAccountID) {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertTrue(store.isAwaitingFreshAccountSkeleton(secondAccountID))
		XCTAssertFalse(
			store.accounts.first(where: {
				$0.account.accountID == secondAccountID
			})?.isRefreshing == true
		)

		await client.releaseSnapshot()
		for _ in 0 ..< 200 {
			let state = store.accounts.first(where: {
				$0.account.accountID == secondAccountID
			})
			if state?.account.accountRevision == 8,
				state?.inventory?.accountRevision == 8,
				store.isAwaitingFreshAccountSkeleton(secondAccountID) == false
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let finalState = try XCTUnwrap(
			store.accounts.first(where: {
				$0.account.accountID == secondAccountID
			})
		)
		XCTAssertEqual(finalState.account.accountRevision, 8)
		XCTAssertEqual(finalState.inventory?.accountRevision, 8)
		XCTAssertFalse(finalState.isRefreshing)
		XCTAssertFalse(store.isAwaitingFreshAccountSkeleton(secondAccountID))
		let reads = await client.readCounts()
		XCTAssertGreaterThanOrEqual(reads.snapshot, 3)
	}

	func testRoutingCompletesWhileDetailReadIsPendingThenReconcilesTheAdvancedAccount() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsInventory: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refreshTask = Task { await store.refresh() }
		var inventoryIsPending = false
		for _ in 0 ..< 200 {
			if await client.inventoryIsPending() {
				inventoryIsPending = true
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard inventoryIsPending else {
			await client.releaseInventory()
			await refreshTask.value
			return XCTFail("Inventory read did not enter the pending state.")
		}

		XCTAssertTrue(store.isRefreshing)
		XCTAssertEqual(store.accounts.map(\.account.accountID), [accountID])

		await store.routeAccount(accountID)

		XCTAssertTrue(store.isRefreshing)
		XCTAssertEqual(
			store.routing,
			AccountRoutingControl(
				revision: 10,
				mode: .fixed(accountID: accountID),
				order: [accountID]
			)
		)
		let readsWhilePending = await client.readCounts()
		XCTAssertEqual(readsWhilePending.snapshot, 1)
		XCTAssertEqual(readsWhilePending.inventory, 1)
		let routeRequest = await client.routeRequest()
		XCTAssertNotNil(routeRequest)

		await client.releaseInventory()
		await refreshTask.value

		XCTAssertFalse(store.isRefreshing)
		let finalReads = await client.readCounts()
		XCTAssertEqual(finalReads.snapshot, 1)
		XCTAssertEqual(finalReads.inventory, 2)
	}

	func testEnrollmentStartsAfterSkeletonPublishesWhileDetailReadIsPending() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsInventory: true,
			reauthenticationStates: [.completed]
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .zero,
			openLoginURL: { _ in }
		)

		let refreshTask = Task { await store.refresh() }
		for _ in 0 ..< 200 {
			if await client.inventoryIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertTrue(store.isRefreshing)
		XCTAssertTrue(store.refreshSkeletonIsPublished)
		XCTAssertTrue(store.hasLoaded)
		XCTAssertFalse(store.isInitialLoading)
		XCTAssertTrue(store.canBeginEnrollment)

		store.beginAccountEnrollment()
		XCTAssertEqual(store.accountReauthentication?.phase, .selectingMethod)
		let requestBeforeSelection = await client.enrollmentLoginStartRequest()
		XCTAssertNil(requestBeforeSelection)
		store.selectAccountLoginMethod(.browserRedirect)
		for _ in 0 ..< 200 {
			if await client.enrollmentLoginStartRequest() != nil {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let enrollmentRequest = await client.enrollmentLoginStartRequest()
		let sharedImportRequestCount = await client.enrollmentRequestCount()
		XCTAssertTrue(store.isRefreshing)
		XCTAssertNotNil(enrollmentRequest)
		XCTAssertEqual(enrollmentRequest?.loginMethod, .browserRedirect)
		XCTAssertEqual(sharedImportRequestCount, 0)

		await client.releaseInventory()
		await refreshTask.value
		for _ in 0 ..< 200 {
			if store.accountReauthentication == nil, store.accounts.count == 2 {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertFalse(store.isRefreshing)
		XCTAssertEqual(store.accounts.count, 2)
		XCTAssertEqual(store.message?.tone, .success)
		XCTAssertEqual(store.message?.text, "Account added.")
	}

	func testBrowserEnrollmentRefreshesTheRestoredAccountIdentityAfterLogout() async throws {
		let restoredAccountID = "22222222-2222-4222-8222-222222222222"
		let client = AccountControlStoreClient(
			account: accountRecord(),
			authority: authority,
			reauthenticationStates: [.completed],
			restoredEnrollmentAccountID: restoredAccountID
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .zero,
			accountObservationRetryDelays: [],
			openLoginURL: { _ in }
		)

		await store.refresh()
		store.beginAccountEnrollment()
		store.selectAccountLoginMethod(.browserRedirect)
		for _ in 0 ..< 200 {
			if store.accountReauthentication == nil,
				store.accounts.contains(where: { $0.account.accountID == restoredAccountID })
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let enrollmentRequest = await client.enrollmentLoginStartRequest()
		let requestedAccountID = try XCTUnwrap(enrollmentRequest?.accountID)
		XCTAssertNotEqual(requestedAccountID, restoredAccountID)
		XCTAssertTrue(store.accounts.contains(where: {
			$0.account.accountID == restoredAccountID
				&& $0.account.accountRevision == 3
				&& $0.account.credentialBinding?.version == 2
		}))
		XCTAssertFalse(store.accounts.contains(where: {
			$0.account.accountID == requestedAccountID
		}))
		XCTAssertEqual(store.message?.tone, .success)
		XCTAssertEqual(store.message?.text, "Account added.")
	}

	func testEnrollmentPickerClosesBeforeStartingNativeLogin() async {
		let client = AccountControlStoreClient(
			account: accountRecord(),
			authority: authority
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		await store.refresh()

		store.beginAccountEnrollment()
		XCTAssertEqual(store.accountReauthentication?.phase, .selectingMethod)

		store.closeAccountReauthentication()

		XCTAssertNil(store.accountReauthentication)
		XCTAssertFalse(store.isEnrollingAccount)
		let startRequest = await client.enrollmentLoginStartRequest()
		let cancelCount = await client.reauthenticationCancelCount()
		XCTAssertNil(startRequest)
		XCTAssertEqual(cancelCount, 0)
	}

	func testDeviceCodeCardActionCopiesTheCodeAndOpensItsVerificationURLOnce() async throws {
		let client = AccountControlStoreClient(
			account: accountRecord(),
			authority: authority,
			reauthenticationStates: [.waitingForBrowser]
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let codeRecorder = LoginCodeRecorder()
		let urlRecorder = LoginURLRecorder()
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .seconds(60),
			copyLoginCode: { codeRecorder.copy($0) },
			openLoginURL: { urlRecorder.open($0) }
		)
		await store.refresh()
		store.beginAccountEnrollment()
		store.selectAccountLoginMethod(.deviceCode)
		for _ in 0 ..< 200 {
			if store.accountReauthentication?.prompt != nil {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		let prompt = try XCTUnwrap(store.accountReauthentication?.prompt)

		store.activateAccountLoginPrompt(prompt)
		store.activateAccountLoginPrompt(
			AccountReauthenticationPrompt(
				verificationURL: prompt.verificationURL,
				userCode: "STAL-E123"
			)
		)

		XCTAssertEqual(codeRecorder.codes, [prompt.userCode])
		XCTAssertEqual(urlRecorder.urls, [prompt.verificationURL])
		await store.cancelAccountReauthentication()
	}

	func testDeviceEnrollmentDuplicateProviderKeepsExistingAccount() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			reauthenticationStates: [.failed],
			accountLoginFailure: .providerAlreadyEnrolled
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .zero
		)

		await store.refresh()
		store.beginAccountEnrollment()
		store.selectAccountLoginMethod(.deviceCode)
		for _ in 0 ..< 200 {
			if store.accountReauthentication?.failureText != nil {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let enrollmentRequest = await client.enrollmentLoginStartRequest()
		let sharedImportRequestCount = await client.enrollmentRequestCount()
		XCTAssertNotNil(enrollmentRequest)
		XCTAssertEqual(enrollmentRequest?.loginMethod, .deviceCode)
		XCTAssertEqual(sharedImportRequestCount, 0)
		XCTAssertEqual(store.accounts.map { $0.account.accountID }, [accountID])
		XCTAssertEqual(store.accountReauthentication?.mode, .enrollment)
		XCTAssertFalse(store.isEnrollingAccount)
		XCTAssertEqual(
			store.accountReauthentication?.failureText,
			"This Codex login is already added. Choose a different account on the login page, then try again."
		)
	}

	func testSkeletonPublishesWhileProjectionReadIsPending() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsProjection: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refreshTask = Task { await store.refresh() }
		for _ in 0 ..< 200 {
			if await client.projectionIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertTrue(store.hasLoaded)
		XCTAssertFalse(store.isInitialLoading)
		XCTAssertEqual(store.accounts.map(\.account.accountID), [accountID])

		await client.releaseProjection()
		await refreshTask.value
		XCTAssertFalse(store.isRefreshing)
	}

	func testStartupReadCannotPublishProjectionCapturedBeforeCommittedRouteSnapshot() async throws {
		let targetAccountID = "22222222-2222-4222-8222-222222222222"
		let sourceAccount = accountRecord()
		let targetAccount = accountRecord(
			accountID: targetAccountID,
			alias: "Account 00000-00002"
		)
		let client = AccountControlStoreClient(
			account: sourceAccount,
			secondaryAccount: targetAccount,
			authority: authority,
			suspendsSnapshot: true,
			suspendsProjection: true,
			capturesProjectionBeforeWait: true,
			routing: AccountRoutingControl(
				revision: 9,
				mode: .fixed(accountID: accountID),
				order: [accountID, targetAccountID]
			),
			pendingRoute: AccountRoutePending(
				operationID: "33333333-3333-4333-8333-333333333333",
				accountID: targetAccountID,
				routingRevision: 9
			),
			projection: .current(
				accountID: accountID,
				accountRevision: sourceAccount.accountRevision,
				projectionDigest: String(repeating: "a", count: 64)
			)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refreshTask = Task { await store.refresh() }
		for _ in 0 ..< 200 {
			if await client.snapshotIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.snapshotIsPending() else {
			await client.releaseSnapshot()
			await client.releaseProjection()
			await refreshTask.value
			return XCTFail("The startup snapshot did not enter the pending state.")
		}
		for _ in 0 ..< 200 {
			if await client.projectionReadCount() > 0 {
				break
			}
			await Task.yield()
		}
		let startupProjectionReadsBeforeCommit = await client.projectionReadCount()
		XCTAssertEqual(
			startupProjectionReadsBeforeCommit,
			0,
			"Projection must not start before the authoritative snapshot returns."
		)

		let committedTarget = accountRecord(
			accountID: targetAccountID,
			alias: "Account 00000-00002",
			revision: 8
		)
		await client.commitRoute(
			account: committedTarget,
			routing: AccountRoutingControl(
				revision: 10,
				mode: .fixed(accountID: targetAccountID),
				order: [accountID, targetAccountID]
			),
			projection: .current(
				accountID: targetAccountID,
				accountRevision: committedTarget.accountRevision,
				projectionDigest: String(repeating: "b", count: 64)
			)
		)
		await client.releaseSnapshot()
		await client.releaseProjection()
		await refreshTask.value

		XCTAssertEqual(store.routing?.mode, .fixed(accountID: targetAccountID))
		XCTAssertNil(store.pendingRoute)
		XCTAssertTrue(store.isCodexProjection(targetAccountID))
		XCTAssertFalse(store.isCodexProjection(accountID))
		XCTAssertFalse(store.canRouteAccount(targetAccountID))

		await store.routeAccount(targetAccountID)
		let routeRequest = await client.routeRequest()
		XCTAssertNil(routeRequest, "The committed target must not send a second Route.")
	}

	func testBackgroundSkeletonCannotPublishProjectionCapturedBeforeCommittedRouteSnapshot()
		async throws
	{
		let targetAccountID = "22222222-2222-4222-8222-222222222222"
		let sourceAccount = accountRecord()
		let targetAccount = accountRecord(
			accountID: targetAccountID,
			alias: "Account 00000-00002"
		)
		let client = AccountControlStoreClient(
			account: sourceAccount,
			secondaryAccount: targetAccount,
			authority: authority,
			suspendsSnapshotAfterFirstRead: true,
			suspendsProjectionAfterFirstRead: true,
			capturesProjectionBeforeWait: true,
			routing: AccountRoutingControl(
				revision: 9,
				mode: .fixed(accountID: accountID),
				order: [accountID, targetAccountID]
			),
			projection: .current(
				accountID: accountID,
				accountRevision: sourceAccount.accountRevision,
				projectionDigest: String(repeating: "a", count: 64)
			)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.logoutAccount(accountID)
		for _ in 0 ..< 200 {
			if await client.snapshotIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.snapshotIsPending() else {
			await client.releaseSnapshot()
			await client.releaseProjection()
			return XCTFail("The background skeleton snapshot did not enter the pending state.")
		}
		for _ in 0 ..< 200 {
			if await client.projectionReadCount() > 1 {
				break
			}
			await Task.yield()
		}
		let skeletonProjectionReadsBeforeCommit = await client.projectionReadCount()
		XCTAssertEqual(
			skeletonProjectionReadsBeforeCommit,
			1,
			"Background projection must wait for its authoritative snapshot."
		)

		let committedTarget = accountRecord(
			accountID: targetAccountID,
			alias: "Account 00000-00002",
			revision: 8
		)
		await client.commitRoute(
			account: committedTarget,
			routing: AccountRoutingControl(
				revision: 10,
				mode: .fixed(accountID: targetAccountID),
				order: [accountID, targetAccountID]
			),
			projection: .current(
				accountID: targetAccountID,
				accountRevision: committedTarget.accountRevision,
				projectionDigest: String(repeating: "b", count: 64)
			)
		)
		await client.releaseSnapshot()
		await client.releaseProjection()
		for _ in 0 ..< 200 {
			if store.isRefreshingAccountSkeleton == false,
				await client.projectionReadCount() >= 2
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertEqual(store.routing?.mode, .fixed(accountID: targetAccountID))
		XCTAssertNil(store.pendingRoute)
		XCTAssertTrue(store.isCodexProjection(targetAccountID))
		XCTAssertFalse(store.isCodexProjection(accountID))
		XCTAssertFalse(store.canRouteAccount(targetAccountID))

		await store.routeAccount(targetAccountID)
		let routeRequest = await client.routeRequest()
		XCTAssertNil(routeRequest, "The committed target must not send a second Route.")
	}

	func testDepletedAccountCanBeDisabledWhileDetailReadIsPending() async throws {
		let account = accountRecord(
			observedState: .depleted,
			sevenDayQuota: depletedSevenDayQuota()
		)
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsInventory: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refreshTask = Task { await store.refresh() }
		for _ in 0 ..< 200 {
			if await client.inventoryIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertTrue(store.isRefreshing)
		XCTAssertTrue(store.refreshSkeletonIsPublished)
		XCTAssertEqual(store.accounts.first?.account.sevenDayQuota.usedPercent, 100)

		await store.setAccount(accountID, enabled: false)

		let request = await client.enabledRequest()
		XCTAssertEqual(
			request,
			AccountControlStoreEnabledRequest(
				authority: nil,
				accountID: accountID,
				enabled: false,
				expectedRevision: 7
			)
		)

		await client.releaseInventory()
		await refreshTask.value
	}

	func testDepletedAccountCanBeLoggedOutWhileDetailReadIsPending() async throws {
		let account = accountRecord(
			observedState: .depleted,
			sevenDayQuota: depletedSevenDayQuota()
		)
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsInventory: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refreshTask = Task { await store.refresh() }
		for _ in 0 ..< 200 {
			if await client.inventoryIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertTrue(store.isRefreshing)
		XCTAssertTrue(store.refreshSkeletonIsPublished)
		XCTAssertEqual(store.accounts.first?.account.sevenDayQuota.usedPercent, 100)

		await store.logoutAccount(accountID)

		let request = await client.logoutRequest()
		XCTAssertNil(request?.authority)
		XCTAssertEqual(request?.accountID, accountID)
		XCTAssertEqual(request?.expectedRevision, 7)
		XCTAssertTrue(
			request.map {
				DecodexNativeClient.isCanonicalUUID($0.operationID)
			} ?? false
		)

		await client.releaseInventory()
		await refreshTask.value
	}

	func testRouteCompletesWhileDetailReadIsPending() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsInventory: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		let refreshTask = Task { await store.refresh() }
		for _ in 0 ..< 200 {
			if await client.inventoryIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertTrue(store.isRefreshing)

		await store.routeAccount(accountID)

		XCTAssertTrue(store.isRefreshing)
		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
		XCTAssertNil(store.message)
		let routeRequest = await client.routeRequest()
		XCTAssertEqual(routeRequest?.authority, nil)
		XCTAssertEqual(routeRequest?.accountID, accountID)
		XCTAssertEqual(routeRequest?.expectedAccountRevision, 7)
		XCTAssertTrue(
			routeRequest.map { DecodexNativeClient.isCanonicalUUID($0.idempotencyKey) } ?? false
		)

		await client.releaseInventory()
		await refreshTask.value
	}

	func testMovingAccountsPersistsEachCompleteRoutingOrder() async throws {
		let secondAccountID = "22222222-2222-4222-8222-222222222222"
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			secondaryAccount: accountRecord(
				accountID: secondAccountID,
				alias: "Account 00000-00002"
			),
			authority: authority
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		XCTAssertTrue(store.canReorderAccounts)

		await store.moveAccount(accountID, onto: secondAccountID)

		let order = [secondAccountID, accountID]
		XCTAssertEqual(store.accounts.map { $0.account.accountID }, order)
		XCTAssertEqual(store.routing?.order, order)
		XCTAssertEqual(store.routing?.revision, 10)
		let request = await client.orderRequest()
		XCTAssertEqual(
			request,
			AccountControlStoreOrderRequest(
				authority: authority,
				order: order,
				expectedRoutingRevision: 9
			)
		)
		XCTAssertNil(store.message)

		await store.moveAccount(accountID, onto: secondAccountID)

		let restoredOrder = [accountID, secondAccountID]
		XCTAssertEqual(store.accounts.map { $0.account.accountID }, restoredOrder)
		XCTAssertEqual(store.routing?.order, restoredOrder)
		XCTAssertEqual(store.routing?.revision, 11)
		let secondRequest = await client.orderRequest()
		XCTAssertEqual(
			secondRequest,
			AccountControlStoreOrderRequest(
				authority: authority,
				order: restoredOrder,
				expectedRoutingRevision: 10
			)
		)

		await store.moveAccounts([accountID], before: nil)

		XCTAssertEqual(
			store.accounts.map { $0.account.accountID },
			[secondAccountID, accountID]
		)
		let endRequest = await client.orderRequest()
		XCTAssertEqual(
			endRequest,
			AccountControlStoreOrderRequest(
				authority: authority,
				order: [secondAccountID, accountID],
				expectedRoutingRevision: 11
			)
		)

		await store.moveAccounts([accountID], before: secondAccountID)

		XCTAssertEqual(
			store.accounts.map { $0.account.accountID },
			[accountID, secondAccountID]
		)
		let beforeRequest = await client.orderRequest()
		XCTAssertEqual(
			beforeRequest,
			AccountControlStoreOrderRequest(
				authority: authority,
				order: [accountID, secondAccountID],
				expectedRoutingRevision: 12
			)
		)
	}

	func testRejectedAccountOrderRestoresTheAuthoritativeRows() async throws {
		let secondAccountID = "22222222-2222-4222-8222-222222222222"
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			secondaryAccount: accountRecord(
				accountID: secondAccountID,
				alias: "Account 00000-00002"
			),
			authority: authority,
			accountOrderError: .rejected(
				.staleRoutingControl,
				actualRevision: 10
			)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.moveAccount(accountID, onto: secondAccountID)

		let authoritativeOrder = [accountID, secondAccountID]
		XCTAssertEqual(store.accounts.map { $0.account.accountID }, authoritativeOrder)
		XCTAssertEqual(store.routing?.order, authoritativeOrder)
		XCTAssertEqual(store.routing?.revision, 9)
		XCTAssertNotNil(store.message)
	}

	func testRouteAccountSubmitsOneDaemonCommandAndAppliesOneAuthoritativeResult() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(account: account, authority: authority)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.routeAccount(accountID)

		let recordedRequest = await client.routeRequest()
		let request = try XCTUnwrap(recordedRequest)
		XCTAssertEqual(request.authority, authority)
		XCTAssertEqual(request.accountID, accountID)
		XCTAssertEqual(request.expectedAccountRevision, 7)
		XCTAssertEqual(request.expectedRoutingRevision, 9)
		XCTAssertTrue(DecodexNativeClient.isCanonicalUUID(request.operationID))
		XCTAssertTrue(DecodexNativeClient.isCanonicalUUID(request.idempotencyKey))
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertEqual(store.accounts.first?.account.credentialBinding?.version, 4)
		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
		XCTAssertNil(store.message)
	}

	func testRouteAccountFailureLeavesTheAuthoritativeProjectionUnchanged() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			routeAccountError: .rejected(.lifecycleUnready, actualRevision: nil)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.routeAccount(accountID)

		XCTAssertEqual(store.accounts.first?.account.accountRevision, 7)
		XCTAssertFalse(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .balanced)
		XCTAssertNotNil(store.message)
	}

	func testRouteAccountIsNoOpWhenBothStatesAreCurrent() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			routing: AccountRoutingControl(
				revision: 9,
				mode: .fixed(accountID: accountID),
				order: [accountID]
			),
			projection: .current(
				accountID: accountID,
				accountRevision: 7,
				projectionDigest: String(repeating: "a", count: 64)
			)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		await store.routeAccount(accountID)

		let routeRequest = await client.routeRequest()
		XCTAssertNil(routeRequest)
		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
	}

	func testTransientProjectionReadFailureRetainsCurrentRouteState() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			routing: AccountRoutingControl(
				revision: 9,
				mode: .fixed(accountID: accountID),
				order: [accountID]
			),
			projection: .current(
				accountID: accountID,
				accountRevision: 7,
				projectionDigest: String(repeating: "a", count: 64)
			)
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)

		await store.refresh()
		XCTAssertTrue(store.isCodexProjection(accountID))

		await client.setProjectionError(.applicationUnavailable)
		await store.refresh()

		XCTAssertTrue(
			store.isCodexProjection(accountID),
			"A transient read failure must not flash the current route back to Route."
		)
	}

	func testRouteAccountSerializesSharedProjectionAcrossAccounts() async throws {
		let secondAccountID = "22222222-2222-4222-8222-222222222222"
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			secondaryAccount: accountRecord(
				accountID: secondAccountID,
				alias: "Account 00000-00002"
			),
			authority: authority,
			suspendsRoute: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: []
		)
		await store.refresh()

		let firstRoute = Task {
			await store.routeAccount(accountID)
		}
		for _ in 0 ..< 200 {
			if await client.routeIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		let firstRouteIsPending = await client.routeIsPending()
		XCTAssertTrue(firstRouteIsPending)

		await store.routeAccount(secondAccountID)

		let routeRequest = await client.routeRequest()
		XCTAssertEqual(routeRequest?.accountID, accountID)

		await client.releaseRoute()
		await firstRoute.value

		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertFalse(store.isCodexProjection(secondAccountID))
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
	}

	func testBrowserLoginCompletesAndRefreshesOnlyTheChangedAccountAuthority() async throws {
		let account = accountRecord(observedState: .authFailed)
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			reauthenticationStates: [
				.waitingForBrowser,
				.installing,
				.completed,
			],
			reauthenticationObservationDelayReads: 2
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let loginURLRecorder = LoginURLRecorder()
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .zero,
			accountObservationRetryDelays: [.zero, .zero],
			openLoginURL: { loginURLRecorder.open($0) }
		)
		await store.refresh()
		XCTAssertTrue(store.accounts.first?.requiresLoginRefresh == true)

		store.beginAccountReauthentication(for: accountID)
		store.selectAccountLoginMethod(.browserRedirect)
		for _ in 0 ..< 200 {
			if store.accountReauthentication == nil,
				store.accounts.first?.account.accountRevision == 8,
				store.accounts.first?.account.observedState == .available
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertNil(store.accountReauthentication)
		XCTAssertFalse(store.isControllingAccount(accountID))
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertEqual(store.accounts.first?.account.observedState, .available)
		XCTAssertFalse(store.accounts.first?.requiresLoginRefresh ?? true)
		XCTAssertEqual(store.message?.text, "Account login refreshed.")
		let recordedRequest = await client.reauthenticationStartRequest()
		let request = try XCTUnwrap(recordedRequest)
		XCTAssertEqual(request.accountID, accountID)
		XCTAssertEqual(request.expectedRevision, 7)
		XCTAssertNil(request.recoveryOperationID)
		XCTAssertEqual(request.loginMethod, .browserRedirect)
		XCTAssertEqual(
			loginURLRecorder.urls,
			[URL(string: "https://auth.openai.com/oauth/authorize?fixture=true")!]
		)
		let pollCount = await client.reauthenticationPollCount()
		let cancelCount = await client.reauthenticationCancelCount()
		XCTAssertEqual(pollCount, 2)
		XCTAssertEqual(cancelCount, 0)
		let reads = await client.readCounts()
		XCTAssertGreaterThanOrEqual(reads.snapshot, 3)
		XCTAssertGreaterThanOrEqual(reads.inventory, 4)
	}

	func testAmbiguousRefreshUsesLoginTakeoverWithTheExactRecoveryOperation() async throws {
		let recoveryOperationID = "77777777-7777-4777-8777-777777777777"
		let account = accountRecord(
			observedState: .available,
			lifecycleReadiness: .operationUnsettled,
			unsettledOperation: AccountUnsettledOperation(
				operationID: recoveryOperationID,
				kind: .refresh,
				phase: .recoveryRequired,
				recoveryCode: "provider_refresh_ambiguous"
			)
		)
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			reauthenticationStates: [.waitingForBrowser, .completed]
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .zero
		)
		await store.refresh()

		XCTAssertTrue(store.accounts.first?.requiresLoginRefresh == true)
		XCTAssertEqual(
			store.accounts.first?.loginRefreshRecoveryOperationID,
			recoveryOperationID
		)
		store.beginAccountReauthentication(for: accountID)
		store.selectAccountLoginMethod(.deviceCode)
		for _ in 0 ..< 200 {
			if store.accountReauthentication == nil,
				store.accounts.first?.account.accountRevision == 8
			{
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		let recordedRequest = await client.reauthenticationStartRequest()
		let request = try XCTUnwrap(recordedRequest)
		XCTAssertEqual(request.recoveryOperationID, recoveryOperationID)
		XCTAssertEqual(request.loginMethod, .deviceCode)
		XCTAssertEqual(store.accounts.first?.account.lifecycleReadiness, .ready)
		XCTAssertNil(store.accounts.first?.account.unsettledOperation)
		XCTAssertFalse(store.accounts.first?.requiresLoginRefresh ?? true)
	}

	func testBrowserLoginRemainsActiveUntilExplicitCancel() async throws {
		let account = accountRecord(observedState: .authFailed)
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			reauthenticationStates: [.waitingForBrowser]
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .seconds(60),
			openLoginURL: { _ in }
		)
		await store.refresh()

		store.beginAccountReauthentication(for: accountID)
		store.selectAccountLoginMethod(.deviceCode)
		for _ in 0 ..< 200 {
			if store.accountReauthentication?.phase == .waitingForBrowser {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertEqual(store.accountReauthentication?.phase, .waitingForBrowser)
		XCTAssertTrue(store.isControllingAccount(accountID))

		await store.cancelAccountReauthentication()

		XCTAssertNil(store.accountReauthentication)
		XCTAssertFalse(store.isControllingAccount(accountID))
		let cancelCount = await client.reauthenticationCancelCount()
		XCTAssertEqual(cancelCount, 1)
	}

	func testCancelOutcomeUnknownKeepsFailureWhileReadingBackAuthoritativeState() async throws {
		let account = accountRecord(observedState: .authFailed)
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			reauthenticationStates: [.waitingForBrowser],
			cancelReauthenticationWithOutcomeUnknown: true
		)
		let fixture = pendingFixture()
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .seconds(60),
			openLoginURL: { _ in }
		)
		await store.refresh()

		store.beginAccountReauthentication(for: accountID)
		store.selectAccountLoginMethod(.browserRedirect)
		for _ in 0 ..< 200 {
			if store.accountReauthentication?.phase == .waitingForBrowser {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		await store.cancelAccountReauthentication()

		XCTAssertEqual(
			store.accountReauthentication?.failureText,
			AccountReauthenticationFailure.outcomeUnknown.presentation
		)
		XCTAssertNotEqual(store.message?.text, "Account login refreshed.")
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertEqual(store.accounts.first?.account.observedState, .available)
		XCTAssertFalse(store.accounts.first?.requiresLoginRefresh ?? true)
		let cancelCount = await client.reauthenticationCancelCount()
		XCTAssertEqual(cancelCount, 1)
		let reads = await client.readCounts()
		XCTAssertGreaterThanOrEqual(reads.snapshot, 3)
		XCTAssertGreaterThanOrEqual(reads.inventory, 2)
	}

	private func accountRecord(
		accountID: String? = nil,
		alias: String = "Account 00000-00001",
		revision: UInt64 = 7,
		enabled: Bool = true,
		observedState: ResetCardObservedState = .available,
		lifecycleReadiness: ResetCardLifecycleReadiness = .ready,
		unsettledOperation: AccountUnsettledOperation? = nil,
		fiveHourQuota: ResetCardQuotaWindow = .unknown(durationMinutes: 300),
		sevenDayQuota: ResetCardQuotaWindow = .unknown(durationMinutes: 10_080)
	) -> ResetCardAccountRecord {
		let accountID = accountID ?? self.accountID
		return ResetCardAccountRecord(
			authority: nil,
			accountID: accountID,
			alias: alias,
			accountRevision: revision,
			enabled: enabled,
			observedState: observedState,
			lifecycleReadiness: lifecycleReadiness,
			credentialBinding: AccountCredentialBinding(
				schemaVersion: 1,
				version: 3,
				fingerprintSHA256: String(repeating: "a", count: 64),
				provider: .chatGPT,
				providerAccountID: accountID == self.accountID ? "provider-a" : "provider-b"
			),
			unsettledOperation: unsettledOperation,
			fiveHourQuota: fiveHourQuota,
			sevenDayQuota: sevenDayQuota
		)
	}

	private func depletedSevenDayQuota() -> ResetCardQuotaWindow {
		ResetCardQuotaWindow(
			durationMinutes: 10_080,
			observedAtUnixMicros: 1_000_000,
			state: .current(
				usedPercent: 100,
				resetsAtUnixMicros: 2_000_000
			)
		)
	}

	private func pendingFixture() -> AccountControlPendingFixture {
		let directory = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		return AccountControlPendingFixture(
			directory: directory,
			store: ResetCardPendingAttemptStore(
				journalURL: directory.appendingPathComponent("pending.json")
			)
		)
	}
}

private struct AccountControlStoreRouteRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let operationID: String
	let accountID: String
	let expectedAccountRevision: UInt64
	let expectedRoutingRevision: UInt64
	let idempotencyKey: String
}

private struct AccountControlStoreEnabledRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let accountID: String
	let enabled: Bool
	let expectedRevision: UInt64
}

private struct AccountControlStoreLogoutRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let operationID: String
	let accountID: String
	let expectedRevision: UInt64
}

private struct AccountControlStoreReauthenticationRequest: Equatable, Sendable {
	let accountID: String
	let expectedRevision: UInt64
	let recoveryOperationID: String?
	let loginMethod: AccountLoginMethod
}

private struct AccountControlStoreEnrollmentLoginRequest: Equatable, Sendable {
	let accountID: String
	let enabled: Bool
	let loginMethod: AccountLoginMethod
}

private enum AccountControlStoreActiveLogin: Equatable, Sendable {
	case enrollment(accountID: String, enabled: Bool, loginMethod: AccountLoginMethod)
	case reauthentication(loginMethod: AccountLoginMethod)

	var loginMethod: AccountLoginMethod {
		switch self {
		case .enrollment(_, _, let loginMethod), .reauthentication(let loginMethod):
			return loginMethod
		}
	}
}

private struct AccountControlStoreOrderRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let order: [String]
	let expectedRoutingRevision: UInt64
}

private actor AccountControlStoreClient: AccountControlClient, AccountObservationClient,
	AccountProfileClient {
	private var account: ResetCardAccountRecord
	private var secondaryAccount: ResetCardAccountRecord?
	private let authority: ResetCardAuthority
	private let inventoryGate: AccountControlReadGate?
	private let snapshotGate: AccountControlReadGate?
	private let projectionGate: AccountControlReadGate?
	private let snapshotWaitsAfterFirstRead: Bool
	private let projectionWaitsAfterFirstRead: Bool
	private let capturesSnapshotBeforeWait: Bool
	private let capturesProjectionBeforeWait: Bool
	private let routeGate: AccountControlReadGate?
	private let enrollmentGate: AccountControlReadGate?
	private let inventoryRevisionOverride: UInt64?
	private let maxSuccessfulInventoryReads: Int?
	private var inventoryRevisionsByAccountID = [String: UInt64]()
	private var resetCardStatus: ResetCardOperationState = .notFound
	private let allowsEnrollment: Bool
	private let enrollmentError: AccountControlError?
	private var routing: AccountRoutingControl
	private var routeAccountError: AccountControlError?
	private var pendingRoute: AccountRoutePending?
	private let accountOrderError: AccountControlError?
	private var lastRouteRequest: AccountControlStoreRouteRequest?
	private var lastEnabledRequest: AccountControlStoreEnabledRequest?
	private var lastLogoutRequest: AccountControlStoreLogoutRequest?
	private var lastOrderRequest: AccountControlStoreOrderRequest?
	private var projection: CodexAuthProjection
	private var projectionError: AccountControlError?
	private var projectionReads = 0
	private var profileResults: [AccountProfileRead]
	private var snapshotReadCount = 0
	private var inventoryReadCount = 0
	private var observationGenerations = [UInt64]()
	private var observationContinuation: CheckedContinuation<
		AccountObservationSignal,
		Never
	>?
	private var deliveredObservationGenerationValue: UInt64?
	private var enrollmentRequests = 0
	private var reauthenticationStates: [AccountReauthenticationState]
	private let accountLoginFailure: AccountReauthenticationFailure?
	private var lastReauthenticationStartRequest: AccountControlStoreReauthenticationRequest?
	private var lastEnrollmentLoginStartRequest: AccountControlStoreEnrollmentLoginRequest?
	private var activeLogin: AccountControlStoreActiveLogin?
	private var reauthenticationPolls = 0
	private var reauthenticationCancels = 0
	private var reauthenticationSessionID: String?
	private var reauthenticationCompleted = false
	private var reauthenticationObservationDelayReads: Int
	private let cancelReauthenticationWithOutcomeUnknown: Bool
	private let restoredEnrollmentAccountID: String?

	init(
		account: ResetCardAccountRecord,
		secondaryAccount: ResetCardAccountRecord? = nil,
		authority: ResetCardAuthority,
		suspendsInventory: Bool = false,
		suspendsSnapshot: Bool = false,
		suspendsSnapshotAfterFirstRead: Bool = false,
		suspendsProjection: Bool = false,
		suspendsProjectionAfterFirstRead: Bool = false,
		capturesSnapshotBeforeWait: Bool = false,
		capturesProjectionBeforeWait: Bool = false,
		suspendsRoute: Bool = false,
		suspendsEnrollment: Bool = false,
		inventoryRevisionOverride: UInt64? = nil,
		maxSuccessfulInventoryReads: Int? = nil,
		profileResults: [AccountProfileRead] = [],
		allowsEnrollment: Bool = false,
		enrollmentError: AccountControlError? = nil,
		routing: AccountRoutingControl? = nil,
		routeAccountError: AccountControlError? = nil,
		pendingRoute: AccountRoutePending? = nil,
		accountOrderError: AccountControlError? = nil,
		projection: CodexAuthProjection = .unmanaged,
		reauthenticationStates: [AccountReauthenticationState] = [],
		accountLoginFailure: AccountReauthenticationFailure? = nil,
		reauthenticationObservationDelayReads: Int = 0,
		cancelReauthenticationWithOutcomeUnknown: Bool = false,
		restoredEnrollmentAccountID: String? = nil
	) {
		self.account = account
		self.secondaryAccount = secondaryAccount
		self.authority = authority
		self.projection = projection
		inventoryGate = suspendsInventory ? AccountControlReadGate() : nil
		snapshotGate = suspendsSnapshot || suspendsSnapshotAfterFirstRead
			? AccountControlReadGate()
			: nil
		projectionGate = suspendsProjection || suspendsProjectionAfterFirstRead
			? AccountControlReadGate()
			: nil
		snapshotWaitsAfterFirstRead = suspendsSnapshotAfterFirstRead
		projectionWaitsAfterFirstRead = suspendsProjectionAfterFirstRead
		self.capturesSnapshotBeforeWait = capturesSnapshotBeforeWait
		self.capturesProjectionBeforeWait = capturesProjectionBeforeWait
		routeGate = suspendsRoute ? AccountControlReadGate() : nil
		enrollmentGate = suspendsEnrollment ? AccountControlReadGate() : nil
		self.inventoryRevisionOverride = inventoryRevisionOverride
		self.maxSuccessfulInventoryReads = maxSuccessfulInventoryReads
		self.profileResults = profileResults
		self.allowsEnrollment = allowsEnrollment
		self.enrollmentError = enrollmentError
		self.routing = routing
			?? AccountRoutingControl(
				revision: 9,
				mode: .balanced,
				order: [account.accountID] + (secondaryAccount.map { [$0.accountID] } ?? [])
			)
		self.routeAccountError = routeAccountError
		self.pendingRoute = pendingRoute
		self.accountOrderError = accountOrderError
		self.reauthenticationStates = reauthenticationStates
		self.accountLoginFailure = accountLoginFailure
		self.reauthenticationObservationDelayReads = reauthenticationObservationDelayReads
		self.cancelReauthenticationWithOutcomeUnknown =
			cancelReauthenticationWithOutcomeUnknown
		self.restoredEnrollmentAccountID = restoredEnrollmentAccountID
	}

	func accountSnapshot(
		authority: ResetCardAuthority?
	) async throws -> AccountControlSnapshot {
		if let authority, authority != self.authority {
			throw ResetCardClientError.invalidResponse
		}
		snapshotReadCount += 1
		let availableAccounts = [account] + (secondaryAccount.map { [$0] } ?? [])
		let accountsByID = Dictionary(
			uniqueKeysWithValues: availableAccounts.map { ($0.accountID, $0) }
		)
		let capturedAccounts = routing.order.compactMap { accountsByID[$0] }
		if let snapshotGate,
			snapshotReadCount > (snapshotWaitsAfterFirstRead ? 1 : 0)
		{
			await snapshotGate.wait()
		}
		return AccountControlSnapshot(
			authority: authority,
			accounts: capturesSnapshotBeforeWait
				? capturedAccounts
				: routing.order.compactMap { accountID in
					if account.accountID == accountID {
						return account
					}
					return secondaryAccount?.accountID == accountID
						? secondaryAccount
						: nil
				},
			routing: routing,
			pendingRoute: pendingRoute
		)
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		try await accountSnapshot(authority: authority).accounts
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		inventoryReadCount += 1
		if let maxSuccessfulInventoryReads,
			inventoryReadCount > maxSuccessfulInventoryReads
		{
			throw ResetCardClientError.transportBackpressured
		}
		if let inventoryGate {
			await inventoryGate.wait()
		}
		if reauthenticationCompleted, self.account.observedState == .authFailed {
			if reauthenticationObservationDelayReads > 0 {
				reauthenticationObservationDelayReads -= 1
			} else {
				self.account = ResetCardAccountRecord(
					authority: self.account.authority,
					accountID: self.account.accountID,
					alias: self.account.alias,
					accountRevision: self.account.accountRevision,
					enabled: self.account.enabled,
					observedState: .available,
					lifecycleReadiness: self.account.lifecycleReadiness,
					credentialBinding: self.account.credentialBinding,
					unsettledOperation: self.account.unsettledOperation,
					fiveHourQuota: self.account.fiveHourQuota,
					sevenDayQuota: self.account.sevenDayQuota
				)
			}
		}
		return ResetCardInventory(
			authority: authority,
			accountID: account.accountID,
			accountRevision: inventoryRevisionsByAccountID[account.accountID]
				?? inventoryRevisionOverride
				?? account.accountRevision,
			cards: [],
			fiveHourQuota: account.fiveHourQuota,
			sevenDayQuota: account.sevenDayQuota,
			observationError: nil
		)
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		.notFound
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		resetCardStatus
	}

	func setResetCardStatus(_ status: ResetCardOperationState) {
		resetCardStatus = status
	}

	func setInventoryRevision(_ revision: UInt64, accountID: String) {
		inventoryRevisionsByAccountID[accountID] = revision
	}

	func routeAccount(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedAccountRevision: UInt64,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard DecodexNativeClient.isCanonicalUUID(operationID),
			DecodexNativeClient.isCanonicalUUID(idempotencyKey),
			expectedRoutingRevision == routing.revision
		else {
			throw AccountControlError.invalidInput
		}
		lastRouteRequest = AccountControlStoreRouteRequest(
			authority: authority,
			operationID: operationID,
			accountID: accountID,
			expectedAccountRevision: expectedAccountRevision,
			expectedRoutingRevision: expectedRoutingRevision,
			idempotencyKey: idempotencyKey
		)
		if let routeAccountError {
			throw routeAccountError
		}
		await routeGate?.wait()

		let current: ResetCardAccountRecord
		if account.accountID == accountID {
			current = account
		} else if let secondaryAccount, secondaryAccount.accountID == accountID {
			current = secondaryAccount
		} else {
			throw AccountControlError.invalidInput
		}
		guard current.accountRevision == expectedAccountRevision,
			let binding = current.credentialBinding
		else {
			throw AccountControlError.invalidInput
		}
		let projectionIsCurrent = if case .current(
			let projectedID,
			let projectedRevision,
			_
		) = projection {
			projectedID == accountID && projectedRevision == current.accountRevision
		} else {
			false
		}
		let routed = projectionIsCurrent
			? current
			: ResetCardAccountRecord(
				authority: current.authority,
				accountID: current.accountID,
				alias: current.alias,
				accountRevision: current.accountRevision + 1,
				enabled: current.enabled,
				observedState: current.observedState,
				lifecycleReadiness: current.lifecycleReadiness,
				credentialBinding: AccountCredentialBinding(
					schemaVersion: binding.schemaVersion,
					version: binding.version + 1,
					fingerprintSHA256: String(repeating: "d", count: 64),
					provider: binding.provider,
					providerAccountID: binding.providerAccountID
				),
				unsettledOperation: current.unsettledOperation,
				fiveHourQuota: current.fiveHourQuota,
				sevenDayQuota: current.sevenDayQuota
			)
		if account.accountID == accountID {
			account = routed
		} else {
			secondaryAccount = routed
		}
		if routing.mode != .fixed(accountID: accountID) {
			routing = AccountRoutingControl(
				revision: routing.revision + 1,
				mode: .fixed(accountID: accountID),
				order: routing.order
			)
		}
		let digest = String(repeating: "c", count: 64)
		projection = .current(
			accountID: accountID,
			accountRevision: routed.accountRevision,
			projectionDigest: digest
		)
		pendingRoute = nil
		return .routed(account: routed, routing: routing, projectionDigest: digest)
	}

	func routeRequest() -> AccountControlStoreRouteRequest? {
		lastRouteRequest
	}

	func routeIsPending() async -> Bool {
		guard let routeGate else {
			return false
		}
		return await routeGate.isPending()
	}

	func releaseRoute() async {
		await routeGate?.release()
	}

	func readCounts() -> (snapshot: Int, inventory: Int) {
		(snapshotReadCount, inventoryReadCount)
	}

	func waitForAccountObservation(
		afterGeneration _: UInt64
	) async throws -> AccountObservationSignal {
		if observationGenerations.isEmpty == false {
			let signal = AccountObservationSignal(
				generation: observationGenerations.removeFirst()
			)
			deliveredObservationGenerationValue = signal.generation
			return signal
		}
		return await withCheckedContinuation { continuation in
			observationContinuation = continuation
		}
	}

	func observationIsPending() -> Bool {
		observationContinuation != nil
	}

	func deliveredObservationGeneration() -> UInt64? {
		deliveredObservationGenerationValue
	}

	func publishObservation(generation: UInt64) {
		let signal = AccountObservationSignal(generation: generation)
		if let observationContinuation {
			self.observationContinuation = nil
			deliveredObservationGenerationValue = generation
			observationContinuation.resume(returning: signal)
		} else {
			observationGenerations.append(generation)
		}
	}

	func inventoryIsPending() async -> Bool {
		guard let inventoryGate else {
			return false
		}
		return await inventoryGate.isPending()
	}

	func releaseInventory() async {
		await inventoryGate?.release()
	}

	func snapshotIsPending() async -> Bool {
		guard let snapshotGate else {
			return false
		}
		return await snapshotGate.isPending()
	}

	func releaseSnapshot() async {
		await snapshotGate?.release()
	}

	func replaceAccount(_ account: ResetCardAccountRecord) {
		self.account = account
	}

	func replaceSecondaryAccount(_ account: ResetCardAccountRecord) {
		secondaryAccount = account
	}

	func commitRoute(
		account committedAccount: ResetCardAccountRecord,
		routing committedRouting: AccountRoutingControl,
		projection committedProjection: CodexAuthProjection
	) {
		if account.accountID == committedAccount.accountID {
			account = committedAccount
		} else if secondaryAccount?.accountID == committedAccount.accountID {
			secondaryAccount = committedAccount
		}
		routing = committedRouting
		pendingRoute = nil
		projection = committedProjection
	}

	func enrollFromSharedCodex(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard allowsEnrollment,
			DecodexNativeClient.isCanonicalUUID(operationID),
			DecodexNativeClient.isCanonicalUUID(accountID),
			DecodexNativeClient.isCanonicalUUID(idempotencyKey),
			enabled
		else {
			throw AccountControlError.applicationUnavailable
		}
		enrollmentRequests += 1
		await enrollmentGate?.wait()
		if let enrollmentError {
			throw enrollmentError
		}
		return .accountChanged(account)
	}

	func enrollmentRequestCount() -> Int {
		enrollmentRequests
	}

	func enrollmentIsPending() async -> Bool {
		guard let enrollmentGate else {
			return false
		}
		return await enrollmentGate.isPending()
	}

	func releaseEnrollment() async {
		await enrollmentGate?.release()
	}

	func codexAuthProjection(
		authority: ResetCardAuthority?
	) async throws -> CodexAuthProjection {
		projectionReads += 1
		let capturedProjection = projection
		if let projectionGate,
			projectionReads > (projectionWaitsAfterFirstRead ? 1 : 0)
		{
			await projectionGate.wait()
		}
		if let projectionError {
			throw projectionError
		}
		return capturesProjectionBeforeWait ? capturedProjection : projection
	}

	func setProjectionError(_ error: AccountControlError?) {
		projectionError = error
	}

	func projectionReadCount() -> Int {
		projectionReads
	}

	func projectionIsPending() async -> Bool {
		guard let projectionGate else {
			return false
		}
		return await projectionGate.isPending()
	}

	func releaseProjection() async {
		await projectionGate?.release()
	}

	func profile(
		for _: ResetCardAccountRecord,
		includeEmail _: Bool
	) async throws -> AccountProfileRead {
		guard profileResults.isEmpty == false else {
			throw ResetCardClientError.invalidResponse
		}
		return profileResults.count == 1
			? profileResults[0]
			: profileResults.removeFirst()
	}

	func setAccountEnabled(
		authority: ResetCardAuthority?,
		accountID: String,
		enabled: Bool,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard DecodexNativeClient.isCanonicalUUID(idempotencyKey),
			accountID == account.accountID,
			expectedRevision == account.accountRevision
		else {
			throw AccountControlError.invalidInput
		}
		lastEnabledRequest = AccountControlStoreEnabledRequest(
			authority: authority,
			accountID: accountID,
			enabled: enabled,
			expectedRevision: expectedRevision
		)
		account = ResetCardAccountRecord(
			authority: account.authority,
			accountID: account.accountID,
			alias: account.alias,
			accountRevision: account.accountRevision + 1,
			enabled: enabled,
			observedState: account.observedState,
			lifecycleReadiness: account.lifecycleReadiness,
			credentialBinding: account.credentialBinding,
			unsettledOperation: account.unsettledOperation,
			fiveHourQuota: account.fiveHourQuota,
			sevenDayQuota: account.sevenDayQuota
		)
		return .accountChanged(account)
	}

	func enabledRequest() -> AccountControlStoreEnabledRequest? {
		lastEnabledRequest
	}

	func logoutAccount(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard DecodexNativeClient.isCanonicalUUID(operationID),
			DecodexNativeClient.isCanonicalUUID(idempotencyKey),
			accountID == account.accountID,
			expectedRevision == account.accountRevision
		else {
			throw AccountControlError.invalidInput
		}
		lastLogoutRequest = AccountControlStoreLogoutRequest(
			authority: authority,
			operationID: operationID,
			accountID: accountID,
			expectedRevision: expectedRevision
		)
		return .accountLoggedOut(
			accountID: accountID,
			tombstoneRevision: expectedRevision + 1
		)
	}

	func logoutRequest() -> AccountControlStoreLogoutRequest? {
		lastLogoutRequest
	}

	func setBalancedSelection(
		authority: ResetCardAuthority?,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func setAccountOrder(
		authority: ResetCardAuthority?,
		order: [String],
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard DecodexNativeClient.isCanonicalUUID(idempotencyKey),
			expectedRoutingRevision == routing.revision,
			order.count == routing.order.count,
			Set(order) == Set(routing.order)
		else {
			throw AccountControlError.invalidInput
		}
		lastOrderRequest = AccountControlStoreOrderRequest(
			authority: authority,
			order: order,
			expectedRoutingRevision: expectedRoutingRevision
		)
		if let accountOrderError {
			throw accountOrderError
		}
		routing = AccountRoutingControl(
			revision: routing.revision + 1,
			mode: routing.mode,
			order: order
		)
		return .routingChanged(routing)
	}

	func orderRequest() -> AccountControlStoreOrderRequest? {
		lastOrderRequest
	}

	func startAccountReauthentication(
		authority _: ResetCardAuthority?,
		sessionID: String,
		operationID _: String,
		accountID: String,
		expectedRevision: UInt64,
		recoveryOperationID: String?,
		idempotencyKey _: String,
		loginMethod: AccountLoginMethod
	) async throws -> AccountReauthenticationStatus {
		guard reauthenticationStates.isEmpty == false else {
			throw AccountControlError.applicationUnavailable
		}
		activeLogin = .reauthentication(loginMethod: loginMethod)
		reauthenticationSessionID = sessionID
		lastReauthenticationStartRequest = AccountControlStoreReauthenticationRequest(
			accountID: accountID,
			expectedRevision: expectedRevision,
			recoveryOperationID: recoveryOperationID,
			loginMethod: loginMethod
		)
		let state = reauthenticationStates.removeFirst()
		if state == .completed {
			markAccountLoginCompleted()
		}
		return reauthenticationStatus(
			state: state,
			sessionID: sessionID
		)
	}

	func startAccountEnrollment(
		authority _: ResetCardAuthority?,
		sessionID: String,
		operationID: String,
		accountID: String,
		enabled: Bool,
		idempotencyKey: String,
		loginMethod: AccountLoginMethod
	) async throws -> AccountReauthenticationStatus {
		guard reauthenticationStates.isEmpty == false,
			DecodexNativeClient.isCanonicalUUID(sessionID),
			DecodexNativeClient.isCanonicalUUID(operationID),
			DecodexNativeClient.isCanonicalUUID(accountID),
			DecodexNativeClient.isCanonicalUUID(idempotencyKey)
		else {
			throw AccountControlError.invalidInput
		}
		activeLogin = .enrollment(
			accountID: accountID,
			enabled: enabled,
			loginMethod: loginMethod
		)
		reauthenticationSessionID = sessionID
		lastEnrollmentLoginStartRequest = AccountControlStoreEnrollmentLoginRequest(
			accountID: accountID,
			enabled: enabled,
			loginMethod: loginMethod
		)
		let state = reauthenticationStates.removeFirst()
		if state == .completed {
			markAccountLoginCompleted()
		}
		return reauthenticationStatus(state: state, sessionID: sessionID)
	}

	func pollAccountReauthentication(
		authority _: ResetCardAuthority?,
		sessionID: String
	) async throws -> AccountReauthenticationStatus {
		guard sessionID == reauthenticationSessionID,
			reauthenticationStates.isEmpty == false
		else {
			throw AccountControlError.invalidResponse
		}
		reauthenticationPolls += 1
		let state = reauthenticationStates.removeFirst()
		if state == .completed {
			markAccountLoginCompleted()
		}
		return reauthenticationStatus(state: state, sessionID: sessionID)
	}

	func cancelAccountReauthentication(
		authority _: ResetCardAuthority?,
		sessionID: String
	) async throws -> AccountReauthenticationStatus {
		guard sessionID == reauthenticationSessionID else {
			throw AccountControlError.invalidResponse
		}
		reauthenticationCancels += 1
		if cancelReauthenticationWithOutcomeUnknown {
			markAccountLoginCompleted()
			return reauthenticationStatus(
				state: .failed,
				sessionID: sessionID,
				failure: .outcomeUnknown
			)
		}
		return reauthenticationStatus(state: .cancelled, sessionID: sessionID)
	}

	func reauthenticationStartRequest() -> AccountControlStoreReauthenticationRequest? {
		lastReauthenticationStartRequest
	}

	func enrollmentLoginStartRequest() -> AccountControlStoreEnrollmentLoginRequest? {
		lastEnrollmentLoginStartRequest
	}

	func reauthenticationPollCount() -> Int {
		reauthenticationPolls
	}

	func reauthenticationCancelCount() -> Int {
		reauthenticationCancels
	}

	private func reauthenticationStatus(
		state: AccountReauthenticationState,
		sessionID: String,
		failure: AccountReauthenticationFailure? = nil
	) -> AccountReauthenticationStatus {
		let resolvedAccountID: String? = if state == .completed {
			switch activeLogin {
			case .enrollment(let accountID, _, _):
				restoredEnrollmentAccountID ?? accountID
			case .reauthentication:
				account.accountID
			case .none:
				nil
			}
		} else {
			nil
		}
		return AccountReauthenticationStatus(
			sessionID: sessionID,
			state: state,
			prompt: state == .waitingForBrowser && activeLogin?.loginMethod == .deviceCode
				? AccountReauthenticationPrompt(
					verificationURL: AccountReauthenticationPrompt.verificationURL,
					userCode: "AB12-CDE34"
				)
				: nil,
			authorizationURL: state == .waitingForBrowser
				&& activeLogin?.loginMethod == .browserRedirect
				? URL(string: "https://auth.openai.com/oauth/authorize?fixture=true")
				: nil,
			failure: failure ?? (state == .failed ? accountLoginFailure : nil),
			resolvedAccountID: resolvedAccountID
		)
	}

	private func markAccountLoginCompleted() {
		switch activeLogin {
		case .enrollment(let requestedAccountID, let enabled, _):
			let accountID = restoredEnrollmentAccountID ?? requestedAccountID
			secondaryAccount = ResetCardAccountRecord(
				authority: account.authority,
				accountID: accountID,
				alias: "Account added",
				accountRevision: restoredEnrollmentAccountID == nil ? 1 : 3,
				enabled: enabled,
				observedState: .available,
				lifecycleReadiness: .ready,
				credentialBinding: AccountCredentialBinding(
					schemaVersion: 1,
					version: restoredEnrollmentAccountID == nil ? 1 : 2,
					fingerprintSHA256: String(repeating: "c", count: 64),
					provider: .chatGPT,
					providerAccountID: "provider-added"
				),
				unsettledOperation: nil,
				fiveHourQuota: .unknown(durationMinutes: 300),
				sevenDayQuota: .unknown(durationMinutes: 10_080)
			)
			if routing.order.contains(accountID) == false {
				routing = AccountRoutingControl(
					revision: routing.revision + 1,
					mode: routing.mode,
					order: routing.order + [accountID]
				)
			}
			return
		case .reauthentication, .none:
			break
		}
		reauthenticationCompleted = true
		account = ResetCardAccountRecord(
			authority: account.authority,
			accountID: account.accountID,
			alias: account.alias,
			accountRevision: account.accountRevision + 1,
			enabled: account.enabled,
			observedState: account.observedState,
			lifecycleReadiness: .ready,
			credentialBinding: account.credentialBinding,
			unsettledOperation: nil,
			fiveHourQuota: account.fiveHourQuota,
			sevenDayQuota: account.sevenDayQuota
		)
	}
}

@MainActor
private final class LoginURLRecorder {
	private(set) var urls = [URL]()

	func open(_ url: URL) {
		urls.append(url)
	}
}

@MainActor
private final class LoginCodeRecorder {
	private(set) var codes = [String]()

	func copy(_ code: String) {
		codes.append(code)
	}
}

private actor AccountControlReadGate {
	private var continuations = [CheckedContinuation<Void, Never>]()
	private var isReleased = false

	func wait() async {
		guard isReleased == false else {
			return
		}
		await withCheckedContinuation { continuation in
			continuations.append(continuation)
		}
	}

	func isPending() -> Bool {
		continuations.isEmpty == false
	}

	func release() {
		isReleased = true
		let pending = continuations
		continuations.removeAll()
		for continuation in pending {
			continuation.resume()
		}
	}
}

@MainActor
private struct AccountControlPendingFixture {
	let directory: URL
	let store: ResetCardPendingAttemptStore

	func remove() {
		try? FileManager.default.removeItem(at: directory)
	}
}
