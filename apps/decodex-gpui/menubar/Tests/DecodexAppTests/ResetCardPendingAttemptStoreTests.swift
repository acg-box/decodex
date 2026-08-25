import Foundation
import XCTest
@testable import DecodexApp

@MainActor
final class ResetCardPendingAttemptStoreTests: XCTestCase {
	func testPendingAttemptRoundTripsAcrossStoreInstances() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let attempt = try makeAttempt()

		XCTAssertEqual(fixture.store.insert(attempt), [attempt])

		let reloaded = ResetCardPendingAttemptStore(
			journalURL: fixture.journalURL
		)
		XCTAssertEqual(reloaded.load(), .available([attempt]))
	}

	func testCorruptPendingDocumentBlocksRecoveryAndIsPreserved() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let corrupt = Data("not-json".utf8)
		try corrupt.write(to: fixture.journalURL)

		XCTAssertEqual(fixture.store.load(), .recoveryBlocked([]))
		XCTAssertEqual(try Data(contentsOf: fixture.journalURL), corrupt)
	}

	func testInvalidPendingAttemptIsNotPersisted() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let invalid = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: ResetCardAuthority(
					profileName: "local",
					serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
				),
				accountID: "not-an-account",
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			),
			idempotencyKey: "not-a-key"
		)

		XCTAssertNil(fixture.store.insert(invalid))

		XCTAssertEqual(fixture.store.load(), .available([]))
		XCTAssertFalse(FileManager.default.fileExists(atPath: fixture.journalURL.path))
	}

	func testOversizedPendingDocumentBlocksRecoveryAndIsPreserved() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let oversized = Data(repeating: 0x20, count: 64 * 1_024 + 1)
		try oversized.write(to: fixture.journalURL)

		XCTAssertEqual(fixture.store.load(), .recoveryBlocked([]))
		XCTAssertEqual(try Data(contentsOf: fixture.journalURL), oversized)
	}

	func testDanglingJournalSymlinkBlocksWritesAndIsPreserved() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let missingTarget = fixture.directory.appendingPathComponent("missing-journal")
		try FileManager.default.createSymbolicLink(
			at: fixture.journalURL,
			withDestinationURL: missingTarget
		)

		XCTAssertEqual(fixture.store.load(), .recoveryBlocked([]))
		XCTAssertNil(fixture.store.insert(try makeAttempt()))
		XCTAssertEqual(
			try FileManager.default.destinationOfSymbolicLink(
				atPath: fixture.journalURL.path
			),
			missingTarget.path
		)
		XCTAssertFalse(FileManager.default.fileExists(atPath: missingTarget.path))
	}

	func testSymlinkedOrNonPrivateJournalDirectoryBlocksWrites() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let targetDirectory = fixture.directory.appendingPathComponent(
			"target",
			isDirectory: true
		)
		try FileManager.default.createDirectory(
			at: targetDirectory,
			withIntermediateDirectories: false,
			attributes: [.posixPermissions: 0o700]
		)
		let symlinkDirectory = fixture.directory.appendingPathComponent(
			"linked",
			isDirectory: true
		)
		try FileManager.default.createSymbolicLink(
			at: symlinkDirectory,
			withDestinationURL: targetDirectory
		)
		let linkedStore = ResetCardPendingAttemptStore(
			journalURL: symlinkDirectory.appendingPathComponent("pending.json")
		)

		XCTAssertEqual(linkedStore.load(), .recoveryBlocked([]))
		XCTAssertNil(linkedStore.insert(try makeAttempt()))
		XCTAssertFalse(
			FileManager.default.fileExists(
				atPath: targetDirectory.appendingPathComponent("pending.json").path
			)
		)

		try FileManager.default.setAttributes(
			[.posixPermissions: 0o755],
			ofItemAtPath: targetDirectory.path
		)
		let nonPrivateStore = ResetCardPendingAttemptStore(
			journalURL: targetDirectory.appendingPathComponent("pending.json")
		)
		XCTAssertEqual(nonPrivateStore.load(), .recoveryBlocked([]))
		XCTAssertNil(nonPrivateStore.insert(try makeAttempt()))
		let attributes = try FileManager.default.attributesOfItem(
			atPath: targetDirectory.path
		)
		XCTAssertEqual(
			(attributes[.posixPermissions] as? NSNumber)?.intValue,
			0o755
		)
	}

	func testNonPrivateJournalModeBlocksWritesWithoutChangingTheFile() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let attempt = try makeAttempt()
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o644],
			ofItemAtPath: fixture.journalURL.path
		)
		let original = try Data(contentsOf: fixture.journalURL)

		XCTAssertEqual(fixture.store.load(), .recoveryBlocked([]))
		XCTAssertNil(fixture.store.insert(try makeAttempt(2)))
		XCTAssertNil(fixture.store.remove(attempt))
		XCTAssertEqual(try Data(contentsOf: fixture.journalURL), original)
		let attributes = try FileManager.default.attributesOfItem(
			atPath: fixture.journalURL.path
		)
		XCTAssertEqual(
			(attributes[.posixPermissions] as? NSNumber)?.intValue,
			0o644
		)
	}

	func testLockSymlinkBlocksInsertAndDispatchWithoutChangingItsTarget() async throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let attempt = try makeAttempt()
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		let lockURL = fixture.directory.appendingPathComponent(".pending.json.lock")
		try FileManager.default.removeItem(at: lockURL)
		let lockTarget = fixture.directory.appendingPathComponent("lock-target")
		let marker = Data("do-not-change".utf8)
		try marker.write(to: lockTarget)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o600],
			ofItemAtPath: lockTarget.path
		)
		try FileManager.default.createSymbolicLink(
			at: lockURL,
			withDestinationURL: lockTarget
		)
		var operationRan = false

		let dispatch = await fixture.store.withDispatchLock(
			for: attempt,
			operation: {
				operationRan = true
				return true
			},
			shouldRemove: { $0 }
		)

		XCTAssertNil(dispatch)
		XCTAssertFalse(operationRan)
		XCTAssertNil(fixture.store.insert(try makeAttempt(2)))
		XCTAssertEqual(fixture.store.load(), .available([attempt]))
		XCTAssertEqual(try Data(contentsOf: lockTarget), marker)
		XCTAssertEqual(
			try FileManager.default.destinationOfSymbolicLink(atPath: lockURL.path),
			lockTarget.path
		)
	}

	func testSixtyFifthAttemptIsRejectedWithoutEvictingUnresolvedKeys() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let retained = try (1...ResetCardPendingAttemptStore.maximumAttempts).map(makeAttempt)
		let rejected = try makeAttempt(ResetCardPendingAttemptStore.maximumAttempts + 1)

		for (index, attempt) in retained.enumerated() {
			XCTAssertEqual(
				fixture.store.insert(attempt),
				Array(retained.prefix(index + 1))
			)
		}
		XCTAssertNil(fixture.store.insert(rejected))
		XCTAssertEqual(fixture.store.load(), .available(retained))
	}

	func testAcknowledgedSaveHasPrivateModeAndImmediateFreshReadback() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let attempt = try makeAttempt()

		XCTAssertEqual(fixture.store.insert(attempt), [attempt])

		let attributes = try FileManager.default.attributesOfItem(
			atPath: fixture.journalURL.path
		)
		XCTAssertEqual(
			(attributes[.posixPermissions] as? NSNumber)?.intValue,
			0o600
		)
		XCTAssertEqual(
			ResetCardPendingAttemptStore(journalURL: fixture.journalURL).load(),
			.available([attempt])
		)
	}

	func testWrongSchemaPreservesRecoverableAttemptsWhileBlockingNewWrites() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let attempt = try makeAttempt()
		XCTAssertEqual(fixture.store.insert(attempt), [attempt])
		var document = try XCTUnwrap(
			JSONSerialization.jsonObject(
				with: Data(contentsOf: fixture.journalURL)
			) as? [String: Any]
		)
		document["schema"] = "decodex/reset-card-pending/unknown"
		let changed = try JSONSerialization.data(withJSONObject: document)
		try changed.write(to: fixture.journalURL)

		XCTAssertEqual(
			fixture.store.load(),
			.recoveryBlocked([attempt])
		)
		XCTAssertNil(fixture.store.remove(attempt))
		XCTAssertEqual(try Data(contentsOf: fixture.journalURL), changed)
	}

	func testIndependentStoreInstancesMergeAddsAndRemovalsUnderTheJournalLock() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let first = try makeAttempt(1)
		let second = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: first.target.authority,
				accountID: first.target.accountID,
				expectedRevision: first.target.expectedRevision,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 300,
					expiresAtUnixSeconds: 400
				)
			),
			idempotencyKey: "018f0f9e-7b6e-4a31-8f4c-000000000002"
		)
		let other = ResetCardPendingAttemptStore(journalURL: fixture.journalURL)

		XCTAssertEqual(fixture.store.insert(first), [first])
		XCTAssertEqual(other.insert(second), [first, second])
		XCTAssertEqual(fixture.store.remove(first), [second])
		XCTAssertEqual(other.load(), .available([second]))
	}

	func testIndependentStoreRejectsASecondKeyForTheSameUnresolvedTarget() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let first = try makeAttempt(1)
		let second = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: ResetCardAuthority(
					profileName: "other",
					serverID: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
				),
				accountID: first.target.accountID,
				expectedRevision: first.target.expectedRevision + 1,
				descriptor: first.target.descriptor
			),
			idempotencyKey: "018f0f9e-7b6e-4a31-8f4c-000000000002"
		)

		XCTAssertEqual(fixture.store.insert(first), [first])
		XCTAssertNil(
			ResetCardPendingAttemptStore(journalURL: fixture.journalURL)
				.insert(second)
		)
		XCTAssertEqual(fixture.store.load(), .available([first]))
	}

	func testDuplicateTargetsInAnExistingJournalBlockDispatchRecovery() throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let first = try makeAttempt(1)
		let second = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: ResetCardAuthority(
					profileName: "other",
					serverID: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
				),
				accountID: first.target.accountID,
				expectedRevision: first.target.expectedRevision + 1,
				descriptor: first.target.descriptor
			),
			idempotencyKey: "018f0f9e-7b6e-4a31-8f4c-000000000002"
		)
		XCTAssertEqual(fixture.store.insert(first), [first])
		var document = try XCTUnwrap(
			JSONSerialization.jsonObject(
				with: Data(contentsOf: fixture.journalURL)
			) as? [String: Any]
		)
		var attempts = try XCTUnwrap(document["attempts"] as? [[String: Any]])
		let encodedSecond = try XCTUnwrap(
			JSONSerialization.jsonObject(with: JSONEncoder().encode(second))
				as? [String: Any]
		)
		attempts.append(encodedSecond)
		document["attempts"] = attempts
		let changed = try JSONSerialization.data(withJSONObject: document)
		try changed.write(to: fixture.journalURL)

		XCTAssertEqual(
			fixture.store.load(),
			.recoveryBlocked([first, second])
		)
		XCTAssertNil(fixture.store.insert(try makeAttempt(3)))
		XCTAssertEqual(try Data(contentsOf: fixture.journalURL), changed)
	}

	private func makeAttempt(_ index: Int = 1) throws -> ResetCardUseAttempt {
		ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: ResetCardAuthority(
					profileName: "local",
					serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
				),
				accountID: "018f0f9e-7b6e-4a31-8f4c-1d2e3f405160",
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: Int64(100 + index * 2),
					expiresAtUnixSeconds: Int64(101 + index * 2)
				)
			),
			idempotencyKey: String(
				format: "018f0f9e-7b6e-4a31-8f4c-%012llx",
				UInt64(index)
			)
		)
	}

	private func makeFixture() throws -> Fixture {
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
		let journalURL = directory.appendingPathComponent("pending.json")

		return Fixture(
			directory: directory,
			journalURL: journalURL,
			store: ResetCardPendingAttemptStore(journalURL: journalURL)
		)
	}
}

@MainActor
private struct Fixture {
	let directory: URL
	let journalURL: URL
	let store: ResetCardPendingAttemptStore

	func remove() {
		try? FileManager.default.removeItem(at: directory)
	}
}
