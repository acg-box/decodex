import Foundation
import XCTest
@testable import DecodexApp

@MainActor
final class ResetCardStoreRecoveryTests: XCTestCase {
	func testRefreshResolvesPersistedCompletedOperation() async throws {
		let fixture = try makeFixture(state: #"{"state":"completed","data":{"outcome":"reset"}}"#)
		defer { fixture.remove() }
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)

		await store.refresh()

		XCTAssertEqual(store.pendingAttempts, [])
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(tone: .success, text: "Usage restored.")
		)
		XCTAssertEqual(fixture.pendingStore.load(), .available([]))
	}

	func testRefreshRetainsPersistedAmbiguousOperation() async throws {
		let fixture = try makeFixture(state: #"{"state":"effect_ambiguous"}"#)
		defer { fixture.remove() }
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)

		await store.refresh()

		XCTAssertEqual(store.pendingAttempts, [fixture.attempt])
		XCTAssertNil(store.message)
		XCTAssertEqual(
			store.pendingStatus(for: fixture.attempt),
			ResetCardPendingStatus.checking(
				detail: "The service is reconciling authoritative Reset Card state."
			)
		)
		XCTAssertEqual(fixture.pendingStore.load(), .available([fixture.attempt]))
	}

	func testRefreshRetainsPendingAttemptWhenAuthoritativeStatusIsUnavailable() async throws {
		let fixture = try makeFixture(
			state: #"{"state":"unavailable","data":{"error":"product_state_unavailable"}}"#
		)
		defer { fixture.remove() }
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)

		await store.refresh()

		XCTAssertEqual(store.pendingAttempts, [fixture.attempt])
		XCTAssertEqual(fixture.pendingStore.load(), .available([fixture.attempt]))
		XCTAssertNil(store.message)
		XCTAssertEqual(
			store.pendingStatus(for: fixture.attempt),
			ResetCardPendingStatus.retrying(
				detail: "Authoritative reset-card state is unavailable."
			)
		)
	}

	func testDefinitelyNotDispatchedRetainsThePersistentPendingAttempt() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"definitely_not_dispatched","failure":"configuration_missing"}
			""",
			exitCode: 2
		)
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		await store.refresh()

		let completion = await store.use(fixture.attempt)

		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: false))
		XCTAssertEqual(store.pendingAttempts, [fixture.attempt])
		XCTAssertEqual(fixture.pendingStore.load(), .available([fixture.attempt]))
		XCTAssertNil(store.message)
		XCTAssertEqual(
			store.pendingStatus(for: fixture.attempt),
			ResetCardPendingStatus.retrying(
				detail: "The reset-card request was not dispatched."
			)
		)
	}

	func testPotentiallyDispatchedStatusCheckDoesNotRedispatch() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"potentially_dispatched","failure":"protocol_timeout"}
			""",
			exitCode: 2
		)
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		await store.refresh()
		_ = await store.use(fixture.attempt)

		await store.checkPendingStatus(fixture.attempt)

		XCTAssertEqual(store.pendingAttempts, [fixture.attempt])
		XCTAssertEqual(fixture.pendingStore.load(), .available([fixture.attempt]))
		XCTAssertNil(store.message)
		XCTAssertEqual(
			store.pendingStatus(for: fixture.attempt),
			ResetCardPendingStatus.checking(
				detail: "No durable Reset Card operation was found yet."
			)
		)
		let invocations = try fixture.invocations()
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card use") }.count,
			1
		)
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card status") }.count,
			1
		)
		XCTAssertTrue(
			invocations
				.filter { $0.contains("reset-card use") }
				.allSatisfy { $0.contains(fixture.attempt.idempotencyKey) }
		)
	}

	func testRejectedBeforeAcceptanceRemovesThePersistentPendingAttempt() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"rejected","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"rejected_before_acceptance","error":{"reason":"idempotency_conflict"}}
			""",
			exitCode: 1
		)
		defer { fixture.remove() }
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		await store.refresh()

		let completion = await store.use(fixture.attempt)

		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: true))
		XCTAssertEqual(store.pendingAttempts, [])
		XCTAssertEqual(fixture.pendingStore.load(), .available([]))
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .error,
				text: "The reset-card request was rejected. Refresh and try again."
			)
		)
	}

	func testAmbiguousPendingTargetRejectsANewKeyWithoutDispatch() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"accepted","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"durably_accepted","state":{"state":"prepared","data":{"account_id":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405160","account_revision":7,"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}}}
			""",
			exitCode: 0,
			statusDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"status","outcome":"effect_ambiguous","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","state":{"state":"effect_ambiguous"}}
			"""
		)
		defer { fixture.remove() }
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		await store.refresh()
		let second = ResetCardUseAttempt(
			target: fixture.attempt.target,
			idempotencyKey: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162"
		)

		let completion = await store.use(second)

		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: true))
		XCTAssertEqual(store.pendingAttempts, [fixture.attempt])
		XCTAssertEqual(fixture.pendingStore.load(), .available([fixture.attempt]))
		XCTAssertTrue(store.blocksNewAttempt(for: fixture.attempt.target))
		XCTAssertEqual(
			try fixture.invocations().filter { $0.contains("reset-card use") },
			[]
		)
		XCTAssertNil(store.message)
		XCTAssertEqual(
			store.pendingStatus(for: fixture.attempt),
			ResetCardPendingStatus.checking(
				detail: "This saved Reset Card request is already being checked automatically."
			)
		)
	}

	func testSixtyFifthPendingAttemptIsRejectedBeforeDispatch() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"accepted","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"durably_accepted","state":{"state":"prepared","data":{"account_id":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405160","account_revision":7,"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}}}
			""",
			exitCode: 0
		)
		defer { fixture.remove() }
		let retained = try (1...ResetCardPendingAttemptStore.maximumAttempts).map { index in
			ResetCardUseAttempt(
				target: ResetCardUseTarget(
					authority: fixture.attempt.target.authority,
					accountID: fixture.attempt.target.accountID,
					expectedRevision: 7,
					descriptor: try ResetCardDescriptor(
						grantedAtUnixSeconds: Int64(1_000 + index * 2),
						expiresAtUnixSeconds: Int64(1_001 + index * 2)
					)
				),
				idempotencyKey: String(
					format: "018f0f9e-7b6e-4a31-8f4c-%012llx",
					UInt64(index)
				)
			)
		}
		for attempt in retained {
			XCTAssertNotNil(fixture.pendingStore.insert(attempt))
		}
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		await store.refresh()

		let completion = await store.use(fixture.attempt)

		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: true))
		XCTAssertEqual(store.pendingAttempts, retained)
		XCTAssertEqual(fixture.pendingStore.load(), .available(retained))
		XCTAssertEqual(
			try fixture.invocations().filter { $0.contains("reset-card use") },
			[]
		)
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .error,
				text: "The pending reset-card limit is reached. Resolve an existing request before starting another."
			)
		)
	}

	func testCorruptRecoveryJournalBlocksNewUseWithoutDeletingEvidence() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"accepted","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"durably_accepted","state":{"state":"prepared","data":{"account_id":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405160","account_revision":7,"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}}}
			""",
			exitCode: 0
		)
		defer { fixture.remove() }
		let corrupt = Data("not-json".utf8)
		try corrupt.write(to: fixture.journalURL)
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)

		await store.refresh()
		let completion = await store.use(fixture.attempt)

		XCTAssertTrue(store.isPendingRecoveryBlocked)
		XCTAssertTrue(store.blocksNewAttempt(for: fixture.attempt.target))
		XCTAssertEqual(completion, ResetCardUseCompletion(resolved: true))
		XCTAssertEqual(store.pendingAttempts, [])
		XCTAssertEqual(fixture.pendingStore.load(), .recoveryBlocked([]))
		XCTAssertEqual(try Data(contentsOf: fixture.journalURL), corrupt)
		XCTAssertEqual(
			try fixture.invocations().filter { $0.contains("reset-card use") },
			[]
		)
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .error,
				text: "The pending reset-card recovery journal is invalid or unavailable. New use is blocked. Preserve the journal for manual inspection; no automatic repair is available."
			)
		)
	}

	func testBlockedJournalStillQueriesStatusForARecoverableAttempt() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"accepted","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"durably_accepted","state":{"state":"prepared","data":{"account_id":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405160","account_revision":7,"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}}}
			""",
			exitCode: 0
		)
		defer { fixture.remove() }
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		var document = try XCTUnwrap(
			JSONSerialization.jsonObject(
				with: Data(contentsOf: fixture.journalURL)
			) as? [String: Any]
		)
		document["schema"] = "decodex/reset-card-pending/unknown"
		let changed = try JSONSerialization.data(withJSONObject: document)
		try changed.write(to: fixture.journalURL)
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)

		await store.refresh()
		await store.checkPendingStatus(fixture.attempt)

		XCTAssertTrue(store.isPendingRecoveryBlocked)
		XCTAssertEqual(store.pendingAttempts, [fixture.attempt])
		XCTAssertEqual(fixture.pendingStore.load(), .recoveryBlocked([fixture.attempt]))
		XCTAssertEqual(try Data(contentsOf: fixture.journalURL), changed)
		let invocations = try fixture.invocations()
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card status") }.count,
			2
		)
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card use") },
			[]
		)
	}

	func testPendingStatusCheckKeepsItsPinnedAuthorityAfterActiveProfileChanges() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"accepted","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"durably_accepted","account_id":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405160","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"prepared","data":{"account_id":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405160","account_revision":7,"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}}}
			""",
			exitCode: 1,
			discoveredProfileName: "other",
			discoveredServerID: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
		)
		defer { fixture.remove() }
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let store = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)

		await store.refresh()
		await store.checkPendingStatus(fixture.attempt)

		let invocations = try fixture.invocations()
		let statusInvocations = invocations.filter { $0.contains("reset-card status") }
		let useInvocations = invocations.filter { $0.contains("reset-card use") }
		XCTAssertEqual(statusInvocations.count, 2)
		XCTAssertEqual(useInvocations.count, 0)
		XCTAssertTrue(
			(statusInvocations + useInvocations).allSatisfy {
				$0.contains("--profile local")
					&& $0.contains(
						"--expected-server-id aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
					)
					&& $0.contains("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb") == false
			}
		)
	}

	func testStaleInstanceCannotCheckAfterAnotherInstanceRemovesTheKey() async throws {
		let fixture = try makeSubmissionFixture(
			useDocument: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"018f0f9e-7b6e-4a31-8f4c-1d2e3f405161","dispatch_state":"potentially_dispatched","failure":"protocol_timeout"}
			""",
			exitCode: 2
		)
		defer { fixture.remove() }
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let staleStore = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		await staleStore.refresh()
		XCTAssertEqual(fixture.pendingStore.remove(fixture.attempt), [])

		await staleStore.checkPendingStatus(fixture.attempt)

		XCTAssertEqual(staleStore.pendingAttempts, [])
		let invocations = try fixture.invocations()
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card use") },
			[]
		)
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card status") }.count,
			1
		)
		XCTAssertNil(staleStore.message)
		XCTAssertEqual(staleStore.pendingStatuses, [:])
	}

	func testConcurrentStatusCompletionRemovesTheKeyWithoutRedispatch() async throws {
		let fixture = try makeConcurrentCompletionFixture()
		defer {
			try? Data().write(to: fixture.statusReleaseURL)
			fixture.remove()
		}
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let completingStore = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		let checkingStore = ResetCardStore(
			client: fixture.client,
			pendingStore: ResetCardPendingAttemptStore(
				journalURL: fixture.journalURL
			)
		)

		let completion = Task {
			await completingStore.checkPendingStatus(fixture.attempt)
		}
		try await waitForFile(fixture.statusEnteredURL)

		let concurrentCheck = Task {
			await checkingStore.checkPendingStatus(fixture.attempt)
		}
		await concurrentCheck.value
		XCTAssertNil(checkingStore.message)
		XCTAssertEqual(
			checkingStore.pendingStatus(for: fixture.attempt),
			ResetCardPendingStatus.retrying(
				detail: "Another app instance changed or is checking this saved Reset Card request."
			)
		)

		try Data().write(to: fixture.statusReleaseURL)
		await completion.value

		XCTAssertEqual(completingStore.pendingAttempts, [])
		XCTAssertEqual(fixture.pendingStore.load(), .available([]))
		XCTAssertEqual(
			completingStore.message,
			ResetCardStoreMessage(
				tone: .success,
				text: "Usage restored."
			)
		)

		await checkingStore.checkPendingStatus(fixture.attempt)

		XCTAssertEqual(checkingStore.pendingAttempts, [])
		let invocations = try fixture.invocations()
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card status") }.count,
			1
		)
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card use") }.count,
			0
		)
	}

	private func makeFixture(state: String) throws -> StoreRecoveryFixture {
		let directory = try makePrivateRecoveryDirectory()
		let journalURL = directory.appendingPathComponent("pending.json")
		let pendingStore = ResetCardPendingAttemptStore(journalURL: journalURL)
		let attempt = try recoveryAttempt()
		let recorder = RecoveryInvocationRecorder()
		let status = operationState(from: state)
		let client = RecoveryClient(
			accounts: [],
			inventory: nil,
			recorder: recorder,
			status: { _ in status },
			use: { _ in .prepared }
		)
		return StoreRecoveryFixture(
			directory: directory,
			journalURL: journalURL,
			pendingStore: pendingStore,
			attempt: attempt,
			recorder: recorder,
			client: client
		)
	}

	private func makeSubmissionFixture(
		useDocument: String,
		exitCode: Int32,
		statusDocument: String? = nil,
		discoveredProfileName: String = "local",
		discoveredServerID: String = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	) throws -> StoreRecoveryFixture {
		_ = exitCode
		let directory = try makePrivateRecoveryDirectory()
		let journalURL = directory.appendingPathComponent("pending.json")
		let pendingStore = ResetCardPendingAttemptStore(journalURL: journalURL)
		let attempt = try recoveryAttempt()
		let recorder = RecoveryInvocationRecorder()
		let discoveredAuthority = ResetCardAuthority(
			profileName: discoveredProfileName,
			serverID: discoveredServerID
		)
		let account = recoveryAccount(
			accountID: attempt.target.accountID,
			authority: nil
		)
		let inventory = ResetCardInventory(
			authority: discoveredAuthority,
			accountID: attempt.target.accountID,
			accountRevision: 7,
			cards: [attempt.target.descriptor],
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080),
			observationError: nil
		)
		let status = statusDocument.map(operationState(from:)) ?? .notFound
		let useError: ResetCardClientError?
		let useState: ResetCardOperationState
		if useDocument.contains("definitely_not_dispatched") {
			useError = .useDefinitelyNotDispatched
			useState = .notFound
		} else if useDocument.contains("potentially_dispatched") {
			useError = .usePotentiallyDispatched
			useState = .notFound
		} else if useDocument.contains("rejected_before_acceptance")
			|| useDocument.contains(#""outcome":"rejected""#)
		{
			useError = .commandRejected
			useState = .notFound
		} else if useDocument.contains(#""state":"effect_ambiguous""#) {
			useError = nil
			useState = .effectAmbiguous
		} else {
			useError = nil
			useState = .prepared
		}
		let client = RecoveryClient(
			accounts: [account],
			inventory: inventory,
			recorder: recorder,
			status: { _ in status },
			use: { _ in
				if let useError {
					throw useError
				}
				return useState
			}
		)
		return StoreRecoveryFixture(
			directory: directory,
			journalURL: journalURL,
			pendingStore: pendingStore,
			attempt: attempt,
			recorder: recorder,
			client: client
		)
	}

	private func makeConcurrentCompletionFixture() throws -> ConcurrentCompletionFixture {
		let directory = try makePrivateRecoveryDirectory()
		let journalURL = directory.appendingPathComponent("pending.json")
		let statusEnteredURL = directory.appendingPathComponent("status-entered")
		let statusReleaseURL = directory.appendingPathComponent("status-release")
		let pendingStore = ResetCardPendingAttemptStore(journalURL: journalURL)
		let attempt = try recoveryAttempt()
		let recorder = RecoveryInvocationRecorder()
		let client = RecoveryClient(
			accounts: [],
			inventory: nil,
			recorder: recorder,
			status: { _ in
				try Data().write(to: statusEnteredURL)
				while FileManager.default.fileExists(atPath: statusReleaseURL.path) == false {
					try await Task.sleep(nanoseconds: 10_000_000)
				}
				return .completed(.reset)
			},
			use: { _ in .prepared }
		)
		return ConcurrentCompletionFixture(
			directory: directory,
			journalURL: journalURL,
			pendingStore: pendingStore,
			attempt: attempt,
			recorder: recorder,
			statusEnteredURL: statusEnteredURL,
			statusReleaseURL: statusReleaseURL,
			client: client
		)
	}

	private func makePrivateRecoveryDirectory() throws -> URL {
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
		return directory
	}

	private func recoveryAttempt() throws -> ResetCardUseAttempt {
		ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: ResetCardAuthority(
					profileName: "local",
					serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
				),
				accountID: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405160",
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			),
			idempotencyKey: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405161"
		)
	}

	private func recoveryAccount(
		accountID: String,
		authority: ResetCardAuthority?
	) -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: authority,
			accountID: accountID,
			alias: "Account 00000-00001",
			accountRevision: 7,
			enabled: true,
			observedState: .depleted,
			lifecycleReadiness: .ready,
			credentialBinding: AccountCredentialBinding(
				schemaVersion: 1,
				version: 1,
				fingerprintSHA256: String(repeating: "a", count: 64),
				provider: .chatGPT,
				providerAccountID: "provider-a"
			),
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
		)
	}

	private func operationState(from document: String) -> ResetCardOperationState {
		if document.contains(#""state":"completed""#) {
			return .completed(.reset)
		}
		if document.contains(#""state":"effect_ambiguous""#) {
			return .effectAmbiguous
		}
		if document.contains(#""state":"unavailable""#) {
			return .unavailable(.productStateUnavailable)
		}
		if document.contains(#""state":"prepared""#) {
			return .prepared
		}
		return .notFound
	}

	private func waitForFile(_ url: URL) async throws {
		for _ in 0..<200 {
			if FileManager.default.fileExists(atPath: url.path) {
				return
			}
			try await Task.sleep(nanoseconds: 10_000_000)
		}
		XCTFail("Timed out waiting for \(url.lastPathComponent)")
	}
}

private final class RecoveryInvocationRecorder: @unchecked Sendable {
	private let lock = NSLock()
	private var storage: [String] = []

	func append(_ value: String) {
		lock.withLock {
			storage.append(value)
		}
	}

	var values: [String] {
		lock.withLock { storage }
	}
}

private final class RecoveryClient: ResetCardClient, @unchecked Sendable {
	typealias StatusHandler = @Sendable (ResetCardUseAttempt) async throws
		-> ResetCardOperationState
	typealias UseHandler = @Sendable (ResetCardUseAttempt) async throws
		-> ResetCardOperationState

	private let accountValues: [ResetCardAccountRecord]
	private let inventoryValue: ResetCardInventory?
	private let recorder: RecoveryInvocationRecorder
	private let statusHandler: StatusHandler
	private let useHandler: UseHandler

	init(
		accounts: [ResetCardAccountRecord],
		inventory: ResetCardInventory?,
		recorder: RecoveryInvocationRecorder,
		status: @escaping StatusHandler,
		use: @escaping UseHandler
	) {
		accountValues = accounts
		inventoryValue = inventory
		self.recorder = recorder
		statusHandler = status
		useHandler = use
	}

	func accounts(
		authority: ResetCardAuthority?
	) async throws -> [ResetCardAccountRecord] {
		recorder.append("account list")
		return accountValues
	}

	func inventory(for account: ResetCardAccountRecord) async throws -> ResetCardInventory {
		recorder.append("reset-card list \(account.accountID)")
		guard let inventoryValue else {
			throw ResetCardClientError.service(.accountNotFound)
		}
		return inventoryValue
	}

	func use(_ attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		recorder.append(
			"--profile \(attempt.target.authority.profileName) "
				+ "--expected-server-id \(attempt.target.authority.serverID) "
				+ "reset-card use \(attempt.idempotencyKey)"
		)
		return try await useHandler(attempt)
	}

	func status(for attempt: ResetCardUseAttempt) async throws -> ResetCardOperationState {
		recorder.append(
			"--profile \(attempt.target.authority.profileName) "
				+ "--expected-server-id \(attempt.target.authority.serverID) "
				+ "reset-card status \(attempt.idempotencyKey)"
		)
		return try await statusHandler(attempt)
	}
}

@MainActor
private struct ConcurrentCompletionFixture {
	let directory: URL
	let journalURL: URL
	let pendingStore: ResetCardPendingAttemptStore
	let attempt: ResetCardUseAttempt
	let recorder: RecoveryInvocationRecorder
	let statusEnteredURL: URL
	let statusReleaseURL: URL
	let client: RecoveryClient

	func invocations() throws -> [String] {
		recorder.values
	}

	func remove() {
		try? FileManager.default.removeItem(at: directory)
	}
}

@MainActor
private struct StoreRecoveryFixture {
	let directory: URL
	let journalURL: URL
	let pendingStore: ResetCardPendingAttemptStore
	let attempt: ResetCardUseAttempt
	let recorder: RecoveryInvocationRecorder
	let client: RecoveryClient

	func invocations() throws -> [String] {
		recorder.values
	}

	func remove() {
		try? FileManager.default.removeItem(at: directory)
	}
}
