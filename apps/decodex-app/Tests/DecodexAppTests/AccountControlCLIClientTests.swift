@testable import DecodexApp
import Foundation
import XCTest

final class AccountControlCLIClientTests: XCTestCase {
	private let accountID = "11111111-1111-4111-8111-111111111111"
	private let secondAccountID = "22222222-2222-4222-8222-222222222222"
	private let operationID = "33333333-3333-4333-8333-333333333333"
	private let idempotencyKey = "44444444-4444-4444-8444-444444444444"
	private let serverID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"

	private var authority: ResetCardAuthority {
		ResetCardAuthority(profileName: "local", serverID: serverID)
	}

	func testAccountSnapshotRetainsRoutingAndCredentialNegativeLifecycleFacts() async throws {
		let fixture = try makeFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/cli-account/1","command":"list","outcome":"success","result":{"outcome":"available","data":{"accounts":[\(readyAccountJSON),\(unsettledAccountJSON)],"routing":{"revision":9,"mode":{"mode":"fixed","account_id":"\(secondAccountID)"},"order":["\(secondAccountID)","\(accountID)"]}}}}'
			"""
		)
		defer { fixture.remove() }
		let client = makeClient(fixture)

		let snapshot = try await client.accountSnapshot(authority: authority)

		XCTAssertEqual(snapshot.authority, authority)
		XCTAssertEqual(
			snapshot.routing,
			AccountRoutingControl(
				revision: 9,
				mode: .fixed(accountID: secondAccountID),
				order: [secondAccountID, accountID]
			)
		)
		XCTAssertEqual(snapshot.accounts.map(\.accountID), [secondAccountID, accountID])
		XCTAssertEqual(
			snapshot.accounts[1].credentialBinding,
			AccountCredentialBinding(
				schemaVersion: 1,
				version: 3,
				fingerprintSHA256: String(repeating: "a", count: 64),
				provider: .chatGPT,
				providerAccountID: "provider-a"
			)
		)
		XCTAssertEqual(
			snapshot.accounts[0].unsettledOperation,
			AccountUnsettledOperation(
				operationID: operationID,
				kind: .refresh,
				phase: .recoveryRequired,
				recoveryCode: "provider_identity_changed"
			)
		)
	}

	func testEveryLifecycleAndRoutingCommandBuildsTheExactPinnedCLIArguments() throws {
		XCTAssertEqual(
			try ResetCardCLIClient.enrollFromSharedCodexArguments(
				authority: authority,
				operationID: operationID,
				accountID: accountID,
				displayLabel: "Account A",
				enabled: true,
				idempotencyKey: idempotencyKey
			),
			prefix + [
				"enroll",
				"--operation-id", operationID,
				"--account-id", accountID,
				"--label", "Account A",
				"--enabled", "true",
				"--idempotency-key", idempotencyKey,
			]
		)
		XCTAssertEqual(
			try ResetCardCLIClient.renameAccountArguments(
				authority: authority,
				accountID: accountID,
				displayLabel: "Renamed",
				expectedRevision: 7,
				idempotencyKey: idempotencyKey
			),
			prefix + [
				"rename",
				"--account-id", accountID,
				"--label", "Renamed",
				"--expected-revision", "7",
				"--idempotency-key", idempotencyKey,
			]
		)
		XCTAssertEqual(
			try ResetCardCLIClient.setAccountEnabledArguments(
				authority: authority,
				accountID: accountID,
				enabled: false,
				expectedRevision: 7,
				idempotencyKey: idempotencyKey
			),
			prefix + [
				"disable",
				"--account-id", accountID,
				"--expected-revision", "7",
				"--idempotency-key", idempotencyKey,
			]
		)
		XCTAssertEqual(
			try ResetCardCLIClient.logoutAccountArguments(
				authority: authority,
				operationID: operationID,
				accountID: accountID,
				expectedRevision: 7,
				idempotencyKey: idempotencyKey
			),
			prefix + [
				"logout",
				"--operation-id", operationID,
				"--account-id", accountID,
				"--expected-revision", "7",
				"--idempotency-key", idempotencyKey,
			]
		)
		XCTAssertEqual(
			try ResetCardCLIClient.setFixedSelectionArguments(
				authority: authority,
				accountID: accountID,
				expectedAccountRevision: 7,
				expectedRoutingRevision: 9,
				idempotencyKey: idempotencyKey
			),
			prefix + [
				"set-fixed-selection",
				"--account-id", accountID,
				"--expected-account-revision", "7",
				"--expected-revision", "9",
				"--idempotency-key", idempotencyKey,
			]
		)
		XCTAssertEqual(
			try ResetCardCLIClient.setBalancedSelectionArguments(
				authority: authority,
				expectedRoutingRevision: 9,
				idempotencyKey: idempotencyKey
			),
			prefix + [
				"set-balanced-selection",
				"--expected-revision", "9",
				"--idempotency-key", idempotencyKey,
			]
		)
		XCTAssertEqual(
			try ResetCardCLIClient.refreshAccountCredentialsArguments(
				authority: authority,
				operationID: operationID,
				accountID: accountID,
				expectedRevision: 7,
				idempotencyKey: idempotencyKey
			),
			prefix + [
				"refresh",
				"--operation-id", operationID,
				"--account-id", accountID,
				"--expected-revision", "7",
				"--idempotency-key", idempotencyKey,
			]
		)
	}

	func testCommandBuildersRejectNoncanonicalIdentityRevisionAuthorityAndLabel() throws {
		let uppercaseAccountID = "abcdefab-cdef-4abc-8def-abcdefabcdef".uppercased()
		let invalidAuthority = ResetCardAuthority(
			profileName: "local space",
			serverID: serverID
		)
		for operation in [
			{
				try ResetCardCLIClient.renameAccountArguments(
					authority: self.authority,
					accountID: uppercaseAccountID,
					displayLabel: "Name",
					expectedRevision: 1,
					idempotencyKey: self.idempotencyKey
				)
			},
			{
				try ResetCardCLIClient.renameAccountArguments(
					authority: self.authority,
					accountID: self.accountID,
					displayLabel: "Name",
					expectedRevision: 0,
					idempotencyKey: self.idempotencyKey
				)
			},
			{
				try ResetCardCLIClient.renameAccountArguments(
					authority: invalidAuthority,
					accountID: self.accountID,
					displayLabel: "Name",
					expectedRevision: 1,
					idempotencyKey: self.idempotencyKey
				)
			},
			{
				try ResetCardCLIClient.renameAccountArguments(
					authority: self.authority,
					accountID: self.accountID,
					displayLabel: "bad\nlabel",
					expectedRevision: 1,
					idempotencyKey: self.idempotencyKey
				)
			},
		] {
			XCTAssertThrowsError(try operation()) { error in
				XCTAssertEqual(error as? AccountControlError, .invalidInput)
			}
		}
	}

	func testFixedSelectionDecodesStrictAppliedRoutingResult() async throws {
		let fixture = try makeFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/cli-account/1","command":"command","outcome":"applied","result":{"outcome":"applied","data":{"entity_revision":10,"result":{"name":"account_routing_changed","data":{"routing":{"revision":10,"mode":{"mode":"fixed","account_id":"\(accountID)"},"order":["\(accountID)","\(secondAccountID)"]}}}}}}'
			"""
		)
		defer { fixture.remove() }
		let client = makeClient(fixture)

		let result = try await client.setFixedSelection(
			authority: authority,
			accountID: accountID,
			expectedAccountRevision: 7,
			expectedRoutingRevision: 9,
			idempotencyKey: idempotencyKey
		)

		XCTAssertEqual(
			result,
			.routingChanged(
				AccountRoutingControl(
					revision: 10,
					mode: .fixed(accountID: accountID),
					order: [accountID, secondAccountID]
				)
			)
		)
		XCTAssertEqual(
			try fixture.invocations(),
			[
				prefix + [
					"set-fixed-selection",
					"--account-id", accountID,
					"--expected-account-revision", "7",
					"--expected-revision", "9",
					"--idempotency-key", idempotencyKey,
				],
			]
		)
	}

	func testTypedRejectionRetainsTheCurrentOwningRevision() async throws {
		let fixture = try makeFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/cli-account/1","command":"command","outcome":"rejected","result":{"outcome":"rejected","data":{"error":{"reason":"account_command_rejected","rejection":"stale_routing_control","actual_revision":10}}}}'
			exit 1
			"""
		)
		defer { fixture.remove() }
		let client = makeClient(fixture)

		do {
			_ = try await client.setBalancedSelection(
				authority: authority,
				expectedRoutingRevision: 9,
				idempotencyKey: idempotencyKey
			)
			XCTFail("A rejected routing command must not appear applied.")
		} catch let error as AccountControlError {
			XCTAssertEqual(
				error,
				.rejected(.staleRoutingControl, actualRevision: 10)
			)
		}
	}

	func testAccountCommandRejectsUnknownFieldsAndContradictoryAppliedResults() async throws {
		let documents = [
			"""
			{"schema":"decodex/cli-account/1","command":"command","outcome":"applied","result":{"outcome":"applied","data":{"entity_revision":10,"result":{"name":"account_routing_changed","data":{"routing":{"revision":10,"mode":{"mode":"balanced"},"order":["\(accountID)"],"unexpected":true}}}}}}
			""",
			"""
			{"schema":"decodex/cli-account/1","command":"command","outcome":"applied","result":{"outcome":"applied","data":{"entity_revision":10,"result":{"name":"account_routing_changed","data":{"routing":{"revision":10,"mode":{"mode":"fixed","account_id":"\(accountID)"},"order":["\(accountID)"]}}}}}}
			""",
		]
		for document in documents {
			let fixture = try makeFixture(body: "printf '%s\\n' '\(document)'")
			defer { fixture.remove() }
			let client = makeClient(fixture)

			do {
				_ = try await client.setBalancedSelection(
					authority: authority,
					expectedRoutingRevision: 9,
					idempotencyKey: idempotencyKey
				)
				XCTFail("Unknown fields or a contradictory result must fail closed.")
			} catch let error as AccountControlError {
				XCTAssertEqual(error, .invalidResponse)
			}
		}
	}

	private var prefix: [String] {
		[
			"--profile", "local",
			"--expected-server-id", serverID,
			"--output", "json", "account",
		]
	}

	private var readyAccountJSON: String {
		"""
		{"account_id":"\(accountID)","display_label":"Account A","enabled":true,"account_revision":7,"observed_state":"available","lifecycle_readiness":"ready","credential_binding":{"schema_version":1,"version":3,"fingerprint_sha256":"\(String(repeating: "a", count: 64))","provider":"chatgpt","provider_account_id":"provider-a"},"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}
		"""
	}

	private var unsettledAccountJSON: String {
		"""
		{"account_id":"\(secondAccountID)","display_label":"Account B","enabled":true,"account_revision":8,"observed_state":"unknown","lifecycle_readiness":"operation_unsettled","credential_binding":{"schema_version":1,"version":2,"fingerprint_sha256":"\(String(repeating: "b", count: 64))","provider":"chatgpt","provider_account_id":"provider-b"},"unsettled_operation":{"operation_id":"\(operationID)","kind":"refresh","phase":"recovery_required","recovery_code":"provider_identity_changed"},"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}
		"""
	}

	private func makeClient(_ fixture: AccountControlCLIFixture) -> ResetCardCLIClient {
		ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)
	}

	private func makeFixture(body: String) throws -> AccountControlCLIFixture {
		let directoryURL = FileManager.default.temporaryDirectory
			.appendingPathComponent(UUID().uuidString, isDirectory: true)
		let executableURL = directoryURL.appendingPathComponent("fake-decodex-cli")
		let logURL = directoryURL.appendingPathComponent("arguments.log")
		try FileManager.default.createDirectory(
			at: directoryURL,
			withIntermediateDirectories: true
		)
		let script = """
		#!/bin/sh
		for argument in "$@"; do
			printf '%s\\n' "$argument" >> '\(logURL.path)'
		done
		printf '%s\\n' -- >> '\(logURL.path)'
		\(body)
		"""
		try script.write(to: executableURL, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executableURL.path
		)
		return AccountControlCLIFixture(
			directoryURL: directoryURL,
			executableURL: executableURL,
			logURL: logURL
		)
	}
}

private struct AccountControlCLIFixture {
	let directoryURL: URL
	let executableURL: URL
	let logURL: URL

	func invocations() throws -> [[String]] {
		let lines = try String(contentsOf: logURL, encoding: .utf8)
			.split(separator: "\n", omittingEmptySubsequences: false)
			.map(String.init)
		var invocations = [[String]]()
		var current = [String]()
		for line in lines {
			if line == "--" {
				invocations.append(current)
				current = []
			} else if line.isEmpty == false {
				current.append(line)
			}
		}
		return invocations
	}

	func remove() {
		try? FileManager.default.removeItem(at: directoryURL)
	}
}
