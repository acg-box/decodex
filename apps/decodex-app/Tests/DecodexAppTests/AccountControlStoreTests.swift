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

	func testStoreRetainsRoutingAndUsesBothFixedSelectionRevisions() async throws {
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
		await store.selectFixedAccount(accountID)

		let fixedRequest = await client.fixedRequest()
		XCTAssertEqual(
			fixedRequest,
			AccountControlStoreFixedRequest(
				authority: authority,
				accountID: accountID,
				expectedAccountRevision: 7,
				expectedRoutingRevision: 9
			)
		)
		XCTAssertEqual(
			store.routing,
			AccountRoutingControl(
				revision: 10,
				mode: .fixed(accountID: accountID),
				order: [accountID]
			)
		)
		let readCounts = await client.readCounts()
		XCTAssertEqual(readCounts.snapshot, 1)
		XCTAssertEqual(readCounts.inventory, 1)
		XCTAssertNil(store.message)
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

		await store.selectFixedAccount(accountID)
		await store.setAccount(accountID, enabled: false)
		await store.useAccountInCodex(accountID)

		let blockedFixedRequest = await client.fixedRequest()
		let blockedEnabledRequest = await client.enabledRequest()
		let blockedUseRequest = await client.useRequest()
		XCTAssertNil(blockedFixedRequest)
		XCTAssertNil(blockedEnabledRequest)
		XCTAssertNil(blockedUseRequest)
		XCTAssertEqual(store.routing?.mode, .balanced)
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 7)

		await client.releaseSnapshot()
		await refreshTask.value

		XCTAssertTrue(store.canPerformDirectAccountControl)
		await store.selectFixedAccount(accountID)
		let completedFixedRequest = await client.fixedRequest()
		XCTAssertNotNil(completedFixedRequest)
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
	}

	func testRefreshDoesNotStartWhileRoutingControlIsInProgress() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsFixedSelection: true
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
			await store.selectFixedAccount(accountID)
		}
		var routingIsPending = false
		for _ in 0 ..< 200 {
			if await client.fixedSelectionIsPending() {
				routingIsPending = true
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard routingIsPending else {
			await client.releaseFixedSelection()
			await routingTask.value
			return XCTFail("Routing control did not enter the pending state.")
		}

		XCTAssertTrue(store.isAccountControlInProgress)
		await store.refresh()
		let readsWhileControlIsPending = await client.readCounts()
		XCTAssertEqual(readsWhileControlIsPending.snapshot, 1)

		await client.releaseFixedSelection()
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
			suspendsFixedSelection: true
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
			await store.selectFixedAccount(accountID)
		}
		for _ in 0 ..< 200 {
			if await client.fixedSelectionIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		guard await client.fixedSelectionIsPending() else {
			await client.releaseFixedSelection()
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
			await client.releaseFixedSelection()
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

		await client.releaseFixedSelection()
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

		await store.selectFixedAccount(accountID)
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

	func testAdvancedInventoryWaitsForFreshSkeletonBeforeControls() async throws {
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

		XCTAssertEqual(store.accounts.first?.account.accountRevision, 7)
		XCTAssertEqual(store.accounts.first?.account.alias, "Account 00000-00001")
		XCTAssertTrue(store.accounts.first?.account.enabled == true)
		XCTAssertNil(store.accounts.first?.inventory)
		XCTAssertTrue(store.isAwaitingFreshAccountSkeleton(accountID))
		XCTAssertTrue(store.accounts.first?.isRefreshing == true)

		await store.selectFixedAccount(accountID)
		await store.setAccount(accountID, enabled: false)
		await store.useAccountInCodex(accountID)
		let blockedFixedRequest = await client.fixedRequest()
		let blockedEnabledRequest = await client.enabledRequest()
		let blockedUseRequest = await client.useRequest()
		XCTAssertNil(blockedFixedRequest)
		XCTAssertNil(blockedEnabledRequest)
		XCTAssertNil(blockedUseRequest)

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
				store.accounts.first?.inventory?.accountRevision == 8
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
		XCTAssertTrue(
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

	func testRoutingCompletesWhileDetailReadIsPendingWithoutFullRefresh() async throws {
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

		await store.selectFixedAccount(accountID)

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
		let fixedRequest = await client.fixedRequest()
		XCTAssertNotNil(fixedRequest)

		await client.releaseInventory()
		await refreshTask.value

		XCTAssertFalse(store.isRefreshing)
		let finalReads = await client.readCounts()
		XCTAssertEqual(finalReads.snapshot, 1)
		XCTAssertEqual(finalReads.inventory, 1)
	}

	func testEnrollmentStartsAfterSkeletonPublishesWhileDetailReadIsPending() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			suspendsInventory: true,
			allowsEnrollment: true
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
		XCTAssertTrue(store.canBeginEnrollment)

		await store.enrollFromSharedCodex()

		let enrollmentRequestCount = await client.enrollmentRequestCount()
		XCTAssertTrue(store.isRefreshing)
		XCTAssertEqual(enrollmentRequestCount, 1)
		XCTAssertEqual(store.message?.tone, .success)

		await client.releaseInventory()
		await refreshTask.value
		XCTAssertFalse(store.isRefreshing)
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

	func testUseInCodexCompletesWhileDetailReadIsPendingWithoutChangingRouting() async throws {
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

		await store.useAccountInCodex(accountID)

		XCTAssertTrue(store.isRefreshing)
		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .balanced)
		XCTAssertNil(store.message)
		let useRequest = await client.useRequest()
		XCTAssertEqual(
			useRequest,
			AccountControlStoreUseRequest(
				authority: nil,
				accountID: accountID,
				expectedRevision: 7
			)
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

	func testRouteAccountProjectsThenSelectsFixedRouting() async throws {
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
		await store.routeAccount(accountID)

		let controlSequence = await client.controlSequence()
		let controlCounts = await client.controlCounts()
		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
		XCTAssertEqual(
			controlSequence,
			[.codexProjection, .fixedRouting]
		)
		XCTAssertEqual(controlCounts, .init(fixed: 1, projection: 1))
		XCTAssertNil(store.message)
	}

	func testRouteAccountDoesNotChangeRoutingWhenProjectionFails() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			useAccountError: .applicationUnavailable
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

		let controlSequence = await client.controlSequence()
		let controlCounts = await client.controlCounts()
		XCTAssertFalse(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .balanced)
		XCTAssertEqual(controlSequence, [.codexProjection])
		XCTAssertEqual(controlCounts, .init(fixed: 0, projection: 1))
		XCTAssertNotNil(store.message)
	}

	func testRouteAccountRetriesOnlyRoutingAfterPartialSuccess() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
			fixedSelectionError: .expectedRevisionMismatch(expected: 9, actual: 10)
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

		let firstControlSequence = await client.controlSequence()
		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .balanced)
		XCTAssertEqual(
			firstControlSequence,
			[.codexProjection, .fixedRouting]
		)
		XCTAssertNotNil(store.message)

		await client.setFixedSelectionError(nil)
		await store.routeAccount(accountID)

		let finalControlSequence = await client.controlSequence()
		let finalControlCounts = await client.controlCounts()
		XCTAssertTrue(store.isCodexProjection(accountID))
		XCTAssertEqual(store.routing?.mode, .fixed(accountID: accountID))
		XCTAssertEqual(
			finalControlSequence,
			[.codexProjection, .fixedRouting, .fixedRouting]
		)
		XCTAssertEqual(finalControlCounts, .init(fixed: 2, projection: 1))
		XCTAssertNil(store.message)
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

		let controlSequence = await client.controlSequence()
		let controlCounts = await client.controlCounts()
		XCTAssertEqual(controlSequence, [])
		XCTAssertEqual(controlCounts, .init(fixed: 0, projection: 0))
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
			suspendsFixedSelection: true
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
			if await client.fixedSelectionIsPending() {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		let firstRouteIsPending = await client.fixedSelectionIsPending()
		XCTAssertTrue(firstRouteIsPending)

		await store.routeAccount(secondAccountID)

		let operationsWhileFirstRouteIsPending = await client.controlSequence()
		let useRequest = await client.useRequest()
		XCTAssertEqual(
			operationsWhileFirstRouteIsPending,
			[.codexProjection, .fixedRouting]
		)
		XCTAssertEqual(useRequest?.accountID, accountID)

		await client.releaseFixedSelection()
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
		let store = ResetCardStore(
			client: client,
			pendingStore: fixture.store,
			startupRetryDelays: [],
			accountReauthenticationPollInterval: .zero,
			accountObservationRetryDelays: [.zero, .zero],
			resolveCodexExecutable: {
				"/Applications/ChatGPT.app/Contents/Resources/codex"
			}
		)
		await store.refresh()
		XCTAssertTrue(store.accounts.first?.requiresLoginRefresh == true)

		store.beginAccountReauthentication(for: accountID)
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
		XCTAssertEqual(
			request.codexBin,
			"/Applications/ChatGPT.app/Contents/Resources/codex"
		)
		let pollCount = await client.reauthenticationPollCount()
		let cancelCount = await client.reauthenticationCancelCount()
		XCTAssertEqual(pollCount, 2)
		XCTAssertEqual(cancelCount, 0)
		let reads = await client.readCounts()
		XCTAssertGreaterThanOrEqual(reads.snapshot, 3)
		XCTAssertGreaterThanOrEqual(reads.inventory, 4)
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
			resolveCodexExecutable: {
				"/Applications/ChatGPT.app/Contents/Resources/codex"
			}
		)
		await store.refresh()

		store.beginAccountReauthentication(for: accountID)
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
			resolveCodexExecutable: {
				"/Applications/ChatGPT.app/Contents/Resources/codex"
			}
		)
		await store.refresh()

		store.beginAccountReauthentication(for: accountID)
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
			fiveHourQuota: .unknown(durationMinutes: 300),
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

private struct AccountControlStoreFixedRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let accountID: String
	let expectedAccountRevision: UInt64
	let expectedRoutingRevision: UInt64
}

private struct AccountControlStoreUseRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let accountID: String
	let expectedRevision: UInt64
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
	let codexBin: String
}

private struct AccountControlStoreOrderRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let order: [String]
	let expectedRoutingRevision: UInt64
}

private enum AccountControlStoreOperation: Equatable, Sendable {
	case codexProjection
	case fixedRouting
}

private struct AccountControlStoreCounts: Equatable, Sendable {
	let fixed: Int
	let projection: Int
}

private actor AccountControlStoreClient: AccountControlClient, AccountObservationClient {
	private var account: ResetCardAccountRecord
	private var secondaryAccount: ResetCardAccountRecord?
	private let authority: ResetCardAuthority
	private let inventoryGate: AccountControlReadGate?
	private let snapshotGate: AccountControlReadGate?
	private let capturesSnapshotBeforeWait: Bool
	private let fixedSelectionGate: AccountControlReadGate?
	private let enrollmentGate: AccountControlReadGate?
	private let inventoryRevisionOverride: UInt64?
	private let maxSuccessfulInventoryReads: Int?
	private var inventoryRevisionsByAccountID = [String: UInt64]()
	private var resetCardStatus: ResetCardOperationState = .notFound
	private let allowsEnrollment: Bool
	private var routing: AccountRoutingControl
	private var fixedSelectionError: AccountControlError?
	private let accountOrderError: AccountControlError?
	private let useAccountError: AccountControlError?
	private var lastFixedRequest: AccountControlStoreFixedRequest?
	private var lastUseRequest: AccountControlStoreUseRequest?
	private var lastEnabledRequest: AccountControlStoreEnabledRequest?
	private var lastLogoutRequest: AccountControlStoreLogoutRequest?
	private var lastOrderRequest: AccountControlStoreOrderRequest?
	private var controlOperations = [AccountControlStoreOperation]()
	private var fixedRequestCount = 0
	private var projectionRequestCount = 0
	private var projection: CodexAuthProjection
	private var projectionError: AccountControlError?
	private var projectionReads = 0
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
	private var lastReauthenticationStartRequest: AccountControlStoreReauthenticationRequest?
	private var reauthenticationPolls = 0
	private var reauthenticationCancels = 0
	private var reauthenticationSessionID: String?
	private var reauthenticationCompleted = false
	private var reauthenticationObservationDelayReads: Int
	private let cancelReauthenticationWithOutcomeUnknown: Bool

	init(
		account: ResetCardAccountRecord,
		secondaryAccount: ResetCardAccountRecord? = nil,
		authority: ResetCardAuthority,
		suspendsInventory: Bool = false,
		suspendsSnapshotAfterFirstRead: Bool = false,
		capturesSnapshotBeforeWait: Bool = false,
		suspendsFixedSelection: Bool = false,
		suspendsEnrollment: Bool = false,
		inventoryRevisionOverride: UInt64? = nil,
		maxSuccessfulInventoryReads: Int? = nil,
		allowsEnrollment: Bool = false,
		routing: AccountRoutingControl? = nil,
		fixedSelectionError: AccountControlError? = nil,
		accountOrderError: AccountControlError? = nil,
		useAccountError: AccountControlError? = nil,
		projection: CodexAuthProjection = .unmanaged,
		reauthenticationStates: [AccountReauthenticationState] = [],
		reauthenticationObservationDelayReads: Int = 0,
		cancelReauthenticationWithOutcomeUnknown: Bool = false
	) {
		self.account = account
		self.secondaryAccount = secondaryAccount
		self.authority = authority
		self.projection = projection
		inventoryGate = suspendsInventory ? AccountControlReadGate() : nil
		snapshotGate = suspendsSnapshotAfterFirstRead ? AccountControlReadGate() : nil
		self.capturesSnapshotBeforeWait = capturesSnapshotBeforeWait
		fixedSelectionGate = suspendsFixedSelection ? AccountControlReadGate() : nil
		enrollmentGate = suspendsEnrollment ? AccountControlReadGate() : nil
		self.inventoryRevisionOverride = inventoryRevisionOverride
		self.maxSuccessfulInventoryReads = maxSuccessfulInventoryReads
		self.allowsEnrollment = allowsEnrollment
		self.routing = routing
			?? AccountRoutingControl(
				revision: 9,
				mode: .balanced,
				order: [account.accountID] + (secondaryAccount.map { [$0.accountID] } ?? [])
			)
		self.fixedSelectionError = fixedSelectionError
		self.accountOrderError = accountOrderError
		self.useAccountError = useAccountError
		self.reauthenticationStates = reauthenticationStates
		self.reauthenticationObservationDelayReads = reauthenticationObservationDelayReads
		self.cancelReauthenticationWithOutcomeUnknown =
			cancelReauthenticationWithOutcomeUnknown
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
		if snapshotReadCount > 1, let snapshotGate {
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
			routing: routing
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

	func setFixedSelection(
		authority: ResetCardAuthority?,
		accountID: String,
		expectedAccountRevision: UInt64,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard DecodexNativeClient.isCanonicalUUID(idempotencyKey) else {
			throw AccountControlError.invalidInput
		}
		lastFixedRequest = AccountControlStoreFixedRequest(
			authority: authority,
			accountID: accountID,
			expectedAccountRevision: expectedAccountRevision,
			expectedRoutingRevision: expectedRoutingRevision
		)
		fixedRequestCount += 1
		controlOperations.append(.fixedRouting)
		if let fixedSelectionError {
			throw fixedSelectionError
		}
		await fixedSelectionGate?.wait()
		routing = AccountRoutingControl(
			revision: 10,
			mode: .fixed(accountID: accountID),
			order: routing.order
		)
		return .routingChanged(routing)
	}

	func fixedRequest() -> AccountControlStoreFixedRequest? {
		lastFixedRequest
	}

	func setFixedSelectionError(_ error: AccountControlError?) {
		fixedSelectionError = error
	}

	func controlSequence() -> [AccountControlStoreOperation] {
		controlOperations
	}

	func controlCounts() -> AccountControlStoreCounts {
		AccountControlStoreCounts(
			fixed: fixedRequestCount,
			projection: projectionRequestCount
		)
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

	func fixedSelectionIsPending() async -> Bool {
		guard let fixedSelectionGate else {
			return false
		}
		return await fixedSelectionGate.isPending()
	}

	func releaseFixedSelection() async {
		await fixedSelectionGate?.release()
	}

	func replaceAccount(_ account: ResetCardAccountRecord) {
		self.account = account
	}

	func replaceSecondaryAccount(_ account: ResetCardAccountRecord) {
		secondaryAccount = account
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
		if let projectionError {
			throw projectionError
		}
		return projection
	}

	func setProjectionError(_ error: AccountControlError?) {
		projectionError = error
	}

	func projectionReadCount() -> Int {
		projectionReads
	}

	func useAccountInCodex(
		authority: ResetCardAuthority?,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		guard DecodexNativeClient.isCanonicalUUID(idempotencyKey) else {
			throw AccountControlError.invalidInput
		}
		lastUseRequest = AccountControlStoreUseRequest(
			authority: authority,
			accountID: accountID,
			expectedRevision: expectedRevision
		)
		projectionRequestCount += 1
		controlOperations.append(.codexProjection)
		if let useAccountError {
			throw useAccountError
		}
		let digest = String(repeating: "c", count: 64)
		projection = .current(
			accountID: accountID,
			accountRevision: expectedRevision,
			projectionDigest: digest
		)
		return .codexAuthProjected(
			accountID: accountID,
			accountRevision: expectedRevision,
			projectionDigest: digest
		)
	}

	func useRequest() -> AccountControlStoreUseRequest? {
		lastUseRequest
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
		idempotencyKey _: String,
		codexBin: String
	) async throws -> AccountReauthenticationStatus {
		guard reauthenticationStates.isEmpty == false else {
			throw AccountControlError.applicationUnavailable
		}
		reauthenticationSessionID = sessionID
		lastReauthenticationStartRequest = AccountControlStoreReauthenticationRequest(
			accountID: accountID,
			expectedRevision: expectedRevision,
			codexBin: codexBin
		)
		return reauthenticationStatus(
			state: reauthenticationStates.removeFirst(),
			sessionID: sessionID
		)
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
			markReauthenticationCompleted()
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
			markReauthenticationCompleted()
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
		return AccountReauthenticationStatus(
			sessionID: sessionID,
			state: state,
			failure: failure
		)
	}

	private func markReauthenticationCompleted() {
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
