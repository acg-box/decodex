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
		XCTAssertEqual(store.message?.tone, .success)
	}

	private func accountRecord() -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: nil,
			accountID: accountID,
			displayLabel: "Account A",
			accountRevision: 7,
			enabled: true,
			observedState: .available,
			lifecycleReadiness: .ready,
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

private actor AccountControlStoreClient: AccountControlClient {
	private let account: ResetCardAccountRecord
	private let authority: ResetCardAuthority
	private var routing: AccountRoutingControl
	private var lastFixedRequest: AccountControlStoreFixedRequest?

	init(
		account: ResetCardAccountRecord,
		authority: ResetCardAuthority
	) {
		self.account = account
		self.authority = authority
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
		ResetCardInventory(
			authority: authority,
			accountID: account.accountID,
			accountRevision: account.accountRevision,
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
		guard ResetCardCLIClient.isCanonicalUUID(idempotencyKey) else {
			throw AccountControlError.invalidInput
		}
		lastFixedRequest = AccountControlStoreFixedRequest(
			authority: authority,
			accountID: accountID,
			expectedAccountRevision: expectedAccountRevision,
			expectedRoutingRevision: expectedRoutingRevision
		)
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

	func enrollFromSharedCodex(
		authority: ResetCardAuthority?,
		operationID: String,
		accountID: String,
		displayLabel: String,
		enabled: Bool,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
	}

	func renameAccount(
		authority: ResetCardAuthority?,
		accountID: String,
		displayLabel: String,
		expectedRevision: UInt64,
		idempotencyKey: String
	) async throws -> AccountControlResult {
		throw AccountControlError.applicationUnavailable
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
		throw AccountControlError.applicationUnavailable
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
