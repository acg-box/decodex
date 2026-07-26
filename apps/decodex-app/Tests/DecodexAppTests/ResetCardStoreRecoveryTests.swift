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
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .information,
				text: "Reset-card use is reconciling authoritative state."
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
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .information,
				text: "Authoritative reset-card status is unavailable. Authoritative reset-card state is unavailable."
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
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .error,
				text: "The reset-card request was not dispatched. Resume the pending request with the same operation key."
			)
		)
	}

	func testPotentiallyDispatchedRetainsAndResumesTheSamePersistentAttempt() async throws {
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

		await store.resume(fixture.attempt)

		XCTAssertEqual(store.pendingAttempts, [fixture.attempt])
		XCTAssertEqual(fixture.pendingStore.load(), .available([fixture.attempt]))
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .error,
				text: "The reset-card request may have been dispatched. Resume the pending request to check authoritative state with the same operation key."
			)
		)
		let invocations = try fixture.invocations()
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card use") }.count,
			2
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
		XCTAssertEqual(
			store.message,
			ResetCardStoreMessage(
				tone: .information,
				text: "Resume the pending request for this reset card with its existing operation key."
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
		await store.resume(fixture.attempt)

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

	func testPendingResumeKeepsItsPinnedAuthorityAfterActiveProfileChanges() async throws {
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
		await store.resume(fixture.attempt)

		let invocations = try fixture.invocations()
		let statusInvocations = invocations.filter { $0.contains("reset-card status") }
		let useInvocations = invocations.filter { $0.contains("reset-card use") }
		XCTAssertEqual(statusInvocations.count, 2)
		XCTAssertEqual(useInvocations.count, 1)
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

	func testStaleInstanceCannotResumeAfterAnotherInstanceRemovesTheKey() async throws {
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

		await staleStore.resume(fixture.attempt)

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
		XCTAssertEqual(
			staleStore.message,
			ResetCardStoreMessage(
				tone: .information,
				text: "Another app instance changed or is using this pending reset-card request. Refresh before continuing."
			)
		)
	}

	func testConcurrentRejectionRemovesTheKeyBeforeAnotherStoreCanResume() async throws {
		let fixture = try makeConcurrentRejectionFixture()
		defer {
			try? Data().write(to: fixture.useReleaseURL)
			fixture.remove()
		}
		XCTAssertEqual(fixture.pendingStore.insert(fixture.attempt), [fixture.attempt])
		let rejectingStore = ResetCardStore(
			client: fixture.client,
			pendingStore: fixture.pendingStore
		)
		let resumingStore = ResetCardStore(
			client: fixture.client,
			pendingStore: ResetCardPendingAttemptStore(
				journalURL: fixture.journalURL
			)
		)

		let rejection = Task {
			await rejectingStore.resume(fixture.attempt)
		}
		try await waitForFile(fixture.useEnteredURL)

		let concurrentResume = Task {
			await resumingStore.resume(fixture.attempt)
		}
		await concurrentResume.value
		XCTAssertEqual(
			resumingStore.message,
			ResetCardStoreMessage(
				tone: .information,
				text: "Another app instance changed or is using this pending reset-card request. Refresh before continuing."
			)
		)

		try Data().write(to: fixture.useReleaseURL)
		await rejection.value

		XCTAssertEqual(rejectingStore.pendingAttempts, [])
		XCTAssertEqual(fixture.pendingStore.load(), .available([]))
		XCTAssertEqual(
			rejectingStore.message,
			ResetCardStoreMessage(
				tone: .error,
				text: "The reset-card request was rejected. Refresh and try again."
			)
		)

		await resumingStore.resume(fixture.attempt)

		XCTAssertEqual(resumingStore.pendingAttempts, [])
		let invocations = try fixture.invocations()
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card status") }.count,
			1
		)
		XCTAssertEqual(
			invocations.filter { $0.contains("reset-card use") }.count,
			1
		)
	}

	private func makeFixture(state: String) throws -> StoreRecoveryFixture {
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
		let executable = directory.appendingPathComponent("decodex-cli")
		let idempotencyKey = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405161"
		let script = """
		#!/bin/sh
		case "$*" in
			"--output json reset-card accounts")
				printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"accounts","outcome":"available","authority":{"profile_name":"local","server_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"},"result":{"outcome":"available","data":{"accounts":[]}}}'
				;;
				"--profile local --expected-server-id aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa --output json reset-card status --idempotency-key \(idempotencyKey)")
					printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"status","outcome":"\(outerOutcome(for: state))","idempotency_key":"\(idempotencyKey)","state":\(state)}'
					exit \(expectedExitCode(for: state))
				;;
			*)
				exit 91
				;;
		esac
		"""
		try script.write(to: executable, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executable.path
		)

		let journalURL = directory.appendingPathComponent("pending.json")
		let pendingStore = ResetCardPendingAttemptStore(
			journalURL: journalURL
		)
		let attempt = ResetCardUseAttempt(
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
			idempotencyKey: idempotencyKey
		)

		return StoreRecoveryFixture(
			directory: directory,
			journalURL: journalURL,
			pendingStore: pendingStore,
			attempt: attempt,
			logURL: nil,
			client: ResetCardCLIClient(
				executableURL: executable,
				environment: ["HOME": directory.path],
				timeout: 2
			)
		)
	}

	private func makeSubmissionFixture(
		useDocument: String,
		exitCode: Int32,
		statusDocument: String? = nil,
		discoveredProfileName: String = "local",
		discoveredServerID: String = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	) throws -> StoreRecoveryFixture {
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
		let executable = directory.appendingPathComponent("decodex-cli")
		let logURL = directory.appendingPathComponent("invocations.log")
		let accountID = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405160"
		let idempotencyKey = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405161"
		let statusDocument = statusDocument ?? """
		{"schema":"decodex/reset-card-cli/1","command":"status","outcome":"not_found","idempotency_key":"\(idempotencyKey)","state":{"state":"not_found"}}
		"""
		let script = """
		#!/bin/sh
		printf '%s\\n' "$*" >> '\(logURL.path)'
		case "$*" in
			"--output json reset-card accounts")
				printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"accounts","outcome":"available","authority":{"profile_name":"\(discoveredProfileName)","server_id":"\(discoveredServerID)"},"result":{"outcome":"available","data":{"accounts":[{"account_id":"\(accountID)","display_label":"Account A","account_revision":7,"admission_state":"depleted"}]}}}'
				;;
			"--profile \(discoveredProfileName) --expected-server-id \(discoveredServerID) --output json reset-card list --account \(accountID)")
				printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"list","outcome":"available","authority":{"profile_name":"\(discoveredProfileName)","server_id":"\(discoveredServerID)"},"result":{"outcome":"available","data":{"account_id":"\(accountID)","account_revision":7,"available_count":1,"cards":[{"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}]}}}'
				;;
			"--profile local --expected-server-id aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa --output json reset-card status --idempotency-key \(idempotencyKey)")
				printf '%s\\n' '\(statusDocument)'
				exit 1
				;;
			"--profile local --expected-server-id aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa --output json reset-card use --account \(accountID) --granted-at 100 --expires-at 200 --expected-revision 7 --idempotency-key \(idempotencyKey) --yes")
				printf '%s\\n' '\(useDocument)'
				exit \(exitCode)
				;;
			*)
				exit 91
				;;
		esac
		"""
		try script.write(to: executable, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executable.path
		)

		let journalURL = directory.appendingPathComponent("pending.json")
		let pendingStore = ResetCardPendingAttemptStore(
			journalURL: journalURL
		)
		let attempt = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: ResetCardAuthority(
					profileName: "local",
					serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
				),
				accountID: accountID,
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			),
			idempotencyKey: idempotencyKey
		)

		return StoreRecoveryFixture(
			directory: directory,
			journalURL: journalURL,
			pendingStore: pendingStore,
			attempt: attempt,
			logURL: logURL,
			client: ResetCardCLIClient(
				executableURL: executable,
				environment: ["HOME": directory.path],
				timeout: 2
			)
		)
	}

	private func makeConcurrentRejectionFixture() throws -> ConcurrentRejectionFixture {
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
		let executable = directory.appendingPathComponent("decodex-cli")
		let logURL = directory.appendingPathComponent("invocations.log")
		let useEnteredURL = directory.appendingPathComponent("use-entered")
		let useReleaseURL = directory.appendingPathComponent("use-release")
		let accountID = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405160"
		let idempotencyKey = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405161"
		let script = """
		#!/bin/sh
		printf '%s\\n' "$*" >> '\(logURL.path)'
		case "$*" in
			"--profile local --expected-server-id aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa --output json reset-card status --idempotency-key \(idempotencyKey)")
				printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"status","outcome":"not_found","idempotency_key":"\(idempotencyKey)","state":{"state":"not_found"}}'
				exit 1
				;;
			"--profile local --expected-server-id aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa --output json reset-card use --account \(accountID) --granted-at 100 --expires-at 200 --expected-revision 7 --idempotency-key \(idempotencyKey) --yes")
				: > '\(useEnteredURL.path)'
				while [ ! -e '\(useReleaseURL.path)' ]; do
					sleep 0.01
				done
				printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"rejected","idempotency_key":"\(idempotencyKey)","dispatch_state":"rejected_before_acceptance","error":{"reason":"idempotency_conflict"}}'
				exit 1
				;;
			*)
				exit 91
				;;
		esac
		"""
		try script.write(to: executable, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executable.path
		)

		let journalURL = directory.appendingPathComponent("pending.json")
		let pendingStore = ResetCardPendingAttemptStore(journalURL: journalURL)
		let attempt = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: ResetCardAuthority(
					profileName: "local",
					serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
				),
				accountID: accountID,
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			),
			idempotencyKey: idempotencyKey
		)

		return ConcurrentRejectionFixture(
			directory: directory,
			journalURL: journalURL,
			pendingStore: pendingStore,
			attempt: attempt,
			logURL: logURL,
			useEnteredURL: useEnteredURL,
			useReleaseURL: useReleaseURL,
			client: ResetCardCLIClient(
				executableURL: executable,
				environment: ["HOME": directory.path],
				timeout: 2
			)
		)
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

	private func outerOutcome(for state: String) -> String {
		if state.contains(#""state":"completed""#) {
			return "completed"
		}
		if state.contains(#""state":"unavailable""#) {
			return "unavailable"
		}

		return "effect_ambiguous"
	}

	private func expectedExitCode(for state: String) -> Int32 {
		state.contains(#""state":"completed""#) ? 0
			: state.contains(#""state":"unavailable""#) ? 2 : 1
	}
}

@MainActor
private struct ConcurrentRejectionFixture {
	let directory: URL
	let journalURL: URL
	let pendingStore: ResetCardPendingAttemptStore
	let attempt: ResetCardUseAttempt
	let logURL: URL
	let useEnteredURL: URL
	let useReleaseURL: URL
	let client: ResetCardCLIClient

	func invocations() throws -> [String] {
		try String(contentsOf: logURL, encoding: .utf8)
			.split(separator: "\n")
			.map(String.init)
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
	let logURL: URL?
	let client: ResetCardCLIClient

	func invocations() throws -> [String] {
		guard let logURL else {
			return []
		}

		return try String(contentsOf: logURL, encoding: .utf8)
			.split(separator: "\n")
			.map(String.init)
	}

	func remove() {
		try? FileManager.default.removeItem(at: directory)
	}
}
