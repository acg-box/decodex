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
		await store.refreshCredentials(for: accountID)
		await store.useAccountInCodex(accountID)

		let blockedFixedRequest = await client.fixedRequest()
		let blockedRefreshRequest = await client.refreshRequest()
		let blockedUseRequest = await client.useRequest()
		XCTAssertNil(blockedFixedRequest)
		XCTAssertNil(blockedRefreshRequest)
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
		await store.refreshCredentials(for: accountID)
		await store.useAccountInCodex(accountID)
		let blockedFixedRequest = await client.fixedRequest()
		let blockedRefreshRequest = await client.refreshRequest()
		let blockedUseRequest = await client.useRequest()
		XCTAssertNil(blockedFixedRequest)
		XCTAssertNil(blockedRefreshRequest)
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

	func testRefreshLoginCompletesWhileDetailReadIsPending() async throws {
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

		await store.refreshCredentials(for: accountID)

		XCTAssertTrue(store.isRefreshing)
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		let refreshRequest = await client.refreshRequest()
		XCTAssertNotNil(refreshRequest)
		XCTAssertFalse(store.isControllingAccount(accountID))

		await client.releaseInventory()
		await refreshTask.value
		for _ in 0 ..< 200 {
			if store.accounts.first?.isRefreshing == false {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}
		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
	}

	func testAccountRevisionChangeInvalidatesAndRechecksCodexProjection() async throws {
		let account = accountRecord()
		let client = AccountControlStoreClient(
			account: account,
			authority: authority,
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
		let initialProjectionReads = await client.projectionReadCount()
		XCTAssertEqual(initialProjectionReads, 1)

		await store.refreshCredentials(for: accountID)
		for _ in 0 ..< 200 {
			if await client.projectionReadCount() >= 2 {
				break
			}
			try await Task.sleep(for: .milliseconds(5))
		}

		XCTAssertEqual(store.accounts.first?.account.accountRevision, 8)
		XCTAssertFalse(store.isCodexProjection(accountID))
		XCTAssertEqual(store.codexAuthProjection, .unmanaged)
		let finalProjectionReads = await client.projectionReadCount()
		XCTAssertEqual(finalProjectionReads, 2)
	}

	private func accountRecord(
		alias: String = "Account 00000-00001",
		revision: UInt64 = 7,
		enabled: Bool = true,
		observedState: ResetCardObservedState = .available,
		lifecycleReadiness: ResetCardLifecycleReadiness = .ready
	) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
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
				providerAccountID: "provider-a"
			),
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
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

private struct AccountControlStoreRefreshRequest: Equatable, Sendable {
	let authority: ResetCardAuthority?
	let accountID: String
	let expectedRevision: UInt64
}

private actor AccountControlStoreClient: AccountControlClient {
	private var account: ResetCardAccountRecord
	private let authority: ResetCardAuthority
	private let inventoryGate: AccountControlReadGate?
	private let snapshotGate: AccountControlReadGate?
	private let fixedSelectionGate: AccountControlReadGate?
	private let inventoryRevisionOverride: UInt64?
	private var routing: AccountRoutingControl
	private var lastFixedRequest: AccountControlStoreFixedRequest?
	private var lastUseRequest: AccountControlStoreUseRequest?
	private var lastRefreshRequest: AccountControlStoreRefreshRequest?
	private var projection: CodexAuthProjection
	private var projectionReads = 0
	private var snapshotReadCount = 0
	private var inventoryReadCount = 0

	init(
		account: ResetCardAccountRecord,
		authority: ResetCardAuthority,
		suspendsInventory: Bool = false,
		suspendsSnapshotAfterFirstRead: Bool = false,
		suspendsFixedSelection: Bool = false,
		inventoryRevisionOverride: UInt64? = nil,
		projection: CodexAuthProjection = .unmanaged
	) {
		self.account = account
		self.authority = authority
		self.projection = projection
		inventoryGate = suspendsInventory ? AccountControlReadGate() : nil
		snapshotGate = suspendsSnapshotAfterFirstRead ? AccountControlReadGate() : nil
		fixedSelectionGate = suspendsFixedSelection ? AccountControlReadGate() : nil
		self.inventoryRevisionOverride = inventoryRevisionOverride
		routing = AccountRoutingControl(
			revision: 9,
			mode: .balanced,
			order: [account.accountID]
		)
	}

	func accountSnapshot(
		authority: ResetCardAuthority?
	) async throws -> AccountControlSnapshot {
		if let authority, authority != self.authority {
			throw ResetCardClientError.invalidResponse
		}
		snapshotReadCount += 1
		if snapshotReadCount > 1, let snapshotGate {
			await snapshotGate.wait()
		}
		return AccountControlSnapshot(
			authority: authority,
			accounts: [account],
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
		if let inventoryGate {
			await inventoryGate.wait()
		}
		return ResetCardInventory(
			authority: authority,
			accountID: account.accountID,
			accountRevision: inventoryRevisionOverride ?? account.accountRevision,
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
		.notFound
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

	func readCounts() -> (snapshot: Int, inventory: Int) {
		(snapshotReadCount, inventoryReadCount)
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

	func enrollFromSharedCodex(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func codexAuthProjection(
		authority: ResetCardAuthority?
	) async throws -> CodexAuthProjection {
		projectionReads += 1
		return projection
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
		throw AccountControlError.applicationUnavailable
	}

	func logoutAccount(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func setBalancedSelection(
		authority: ResetCardAuthority?,
		expectedRoutingRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func refreshAccountCredentials(
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
		lastRefreshRequest = AccountControlStoreRefreshRequest(
			authority: authority,
			accountID: accountID,
			expectedRevision: expectedRevision
		)
		account = ResetCardAccountRecord(
			authority: account.authority,
			accountID: account.accountID,
			alias: account.alias,
			accountRevision: account.accountRevision + 1,
			enabled: account.enabled,
			observedState: .available,
			lifecycleReadiness: .ready,
			credentialBinding: account.credentialBinding,
			unsettledOperation: nil,
			fiveHourQuota: account.fiveHourQuota,
			sevenDayQuota: account.sevenDayQuota
		)
		projection = .unmanaged
		return .accountChanged(account)
	}

	func refreshRequest() -> AccountControlStoreRefreshRequest? {
		lastRefreshRequest
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
