@testable import DecodexApp
import Foundation
import XCTest

final class ResetCardCLIClientTests: XCTestCase {
	private let accountID = "11111111-1111-4111-8111-111111111111"
	private let idempotencyKey = "22222222-2222-4222-8222-222222222222"
	private let serverID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"

	private var authority: ResetCardAuthority {
		ResetCardAuthority(profileName: "local", serverID: serverID)
	}

	func testResolverPrefersExplicitCLIThenUsesBundledCLI() {
		let bundleURL = URL(fileURLWithPath: "/Applications/Decodex.app")
		let overrideURL = URL(fileURLWithPath: "/opt/decodex/bin/decodex")
		let bundledURL = bundleURL
			.appendingPathComponent("Contents/Helpers/decodex-cli")

		XCTAssertEqual(
			ResetCardCLIClient.resolveExecutableURL(
				environment: [ResetCardCLIClient.executableOverrideKey: overrideURL.path],
				bundleURL: bundleURL,
				isExecutableFile: { $0 == overrideURL.path || $0 == bundledURL.path }
			),
			overrideURL
		)
		XCTAssertEqual(
			ResetCardCLIClient.resolveExecutableURL(
				environment: [:],
				bundleURL: bundleURL,
				isExecutableFile: { $0 == bundledURL.path }
			),
			bundledURL
		)
	}

	func testInvalidExplicitCLIIsFailClosed() {
		XCTAssertNil(
			ResetCardCLIClient.resolveExecutableURL(
				environment: [ResetCardCLIClient.executableOverrideKey: "/missing/decodex"],
				bundleURL: URL(fileURLWithPath: "/Applications/Decodex.app"),
				isExecutableFile: { $0.hasSuffix("Contents/Helpers/decodex-cli") }
			)
		)
	}

	func testSanitizedEnvironmentKeepsOnlyProcessBasics() {
		let environment = ResetCardCLIClient.sanitizedChildEnvironment(from: [
			"HOME": "/tmp/home",
			"LANG": "en_US.UTF-8",
			"RESET_CARD_TEST_SECRET": "must-not-pass",
			"OPENAI_API_KEY": "must-not-pass",
			ResetCardCLIClient.executableOverrideKey: "/tmp/decodex",
		])

		XCTAssertEqual(environment["HOME"], "/tmp/home")
		XCTAssertEqual(environment["LANG"], "en_US.UTF-8")
		XCTAssertNil(environment["RESET_CARD_TEST_SECRET"])
		XCTAssertNil(environment["OPENAI_API_KEY"])
		XCTAssertNil(environment[ResetCardCLIClient.executableOverrideKey])
	}

	func testFakeCLIReceivesExactPublicArgumentsAndDecodesStableDocuments() async throws {
		let fixture = try makeFixture()
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: [
				"HOME": fixture.directoryURL.path,
				"RESET_CARD_TEST_SECRET": "must-not-pass",
			],
			timeout: 2
		)

		let accounts = try await client.accounts()
		XCTAssertEqual(
			accounts,
			[
				ResetCardAccountRecord(
					authority: authority,
					accountID: accountID,
					displayLabel: "Account A",
					accountRevision: 7,
					admissionState: .depleted
				),
			]
		)

		let inventory = try await client.inventory(for: accounts[0])
		XCTAssertEqual(inventory.authority, authority)
		XCTAssertEqual(inventory.accountID, accountID)
		XCTAssertEqual(inventory.accountRevision, 7)
		XCTAssertEqual(
			inventory.cards,
			[
				try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				),
			]
		)

		let target = ResetCardUseTarget(
			authority: authority,
			accountID: accountID,
			expectedRevision: 7,
			descriptor: inventory.cards[0]
		)
		let state = try await client.use(
			ResetCardUseAttempt(
				target: target,
				idempotencyKey: idempotencyKey
			)
		)
		XCTAssertEqual(state, .completed(.reset))
		let observed = try await client.status(
			for: ResetCardUseAttempt(
				target: target,
				idempotencyKey: idempotencyKey
			)
		)
		XCTAssertEqual(observed, .completed(.reset))

		let invocations = try fixture.invocations()
		XCTAssertEqual(
			invocations,
			[
				["--output", "json", "reset-card", "accounts"],
				[
					"--profile", "local",
					"--expected-server-id", serverID,
					"--output", "json", "reset-card", "list", "--account", accountID,
				],
				[
					"--profile", "local",
					"--expected-server-id", serverID,
					"--output", "json", "reset-card", "use",
					"--account", accountID,
					"--granted-at", "100",
					"--expires-at", "200",
					"--expected-revision", "7",
					"--idempotency-key", idempotencyKey,
					"--yes",
				],
				[
					"--profile", "local",
					"--expected-server-id", serverID,
					"--output", "json", "reset-card", "status",
					"--idempotency-key", idempotencyKey,
				],
			]
		)

		let allArguments = invocations.flatMap { $0 }.joined(separator: " ")
		for forbidden in [
			"app-server",
			"CODEX_HOME",
			"access-token",
			"refresh-token",
			"auth.json",
			"credit-id",
		] {
			XCTAssertFalse(allArguments.contains(forbidden))
		}
	}

	func testWrongSchemaFailsWithoutReflectingResponseContent() async throws {
		let fixture = try makeFixture(schema: "wrong/schema")
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		do {
			_ = try await client.accounts()
			XCTFail("Wrong schema must fail.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .invalidResponse)
			XCTAssertEqual(
				String(reflecting: error),
				"ResetCardClientError.invalidResponse"
			)
		}
	}

	func testCLIExecutionHasABoundedTimeout() async throws {
		let fixture = try makeRawFixture(body: "exec /bin/sleep 2")
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 0.05
		)

		do {
			_ = try await client.accounts()
			XCTFail("A stalled CLI must time out.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .timedOut)
		}
	}

	func testCLIExecutionRejectsOversizedOutput() async throws {
		let fixture = try makeRawFixture(
			body: "/usr/bin/yes x | /usr/bin/head -c 400000"
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		do {
			_ = try await client.accounts()
			XCTFail("Oversized CLI output must fail.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .outputTooLarge)
		}
	}

	func testNonzeroUncertainOutcomeStillDecodesAsTypedState() async throws {
		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"effect_ambiguous","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"effect_ambiguous"}}'
			exit 1
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)
		let state = try await client.use(makeAttempt())
		XCTAssertEqual(state, .effectAmbiguous)
	}

	func testUnavailableStatusIsNotDecodedAsDurableFailure() async throws {
		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"status","outcome":"unavailable","idempotency_key":"\(idempotencyKey)","state":{"state":"unavailable","data":{"error":"product_state_unavailable"}}}'
			exit 2
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		let state = try await client.status(for: makeAttempt())

		XCTAssertEqual(state, .unavailable(.productStateUnavailable))
		XCTAssertNotEqual(state, .failedBeforeEffect(.productStateUnavailable))
	}

	func testUseRejectsAnUnexpectedAccountRevision() async throws {
		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"completed","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":8,"state":{"state":"completed","data":{"outcome":"reset"}}}'
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)
		do {
			_ = try await client.use(makeAttempt())
			XCTFail("A different account revision must fail closed.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .invalidResponse)
		}
	}

	func testUseDecodesDefinitelyNotDispatchedAndRetainsTheEchoedKey() async throws {
		let error = try await useError(
			json: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"\(idempotencyKey)","dispatch_state":"definitely_not_dispatched","failure":"configuration_missing"}
			""",
			exitCode: 2
		)

		XCTAssertEqual(error, .useDefinitelyNotDispatched)
	}

	func testUseDecodesPotentiallyDispatchedAndRetainsTheEchoedKey() async throws {
		let error = try await useError(
			json: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"\(idempotencyKey)","dispatch_state":"potentially_dispatched","failure":"protocol_timeout"}
			""",
			exitCode: 2
		)

		XCTAssertEqual(error, .usePotentiallyDispatched)
	}

	func testUseDecodesRejectedBeforeAcceptanceAsTerminal() async throws {
		let error = try await useError(
			json: """
			{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"rejected","idempotency_key":"\(idempotencyKey)","dispatch_state":"rejected_before_acceptance","error":{"reason":"idempotency_conflict"}}
			""",
			exitCode: 1
		)

		XCTAssertEqual(error, .commandRejected)
	}

	func testUseRejectsMissingOrContradictoryDispatchState() async throws {
		let cases: [(String, Int32)] = [
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"completed","idempotency_key":"\(idempotencyKey)","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"completed","data":{"outcome":"reset"}}}
				""",
				0
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","failure":"protocol_timeout"}
				""",
				2
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"rejected","idempotency_key":"\(idempotencyKey)","dispatch_state":"potentially_dispatched","error":{"reason":"idempotency_conflict"}}
				""",
				1
			),
		]
		for (json, exitCode) in cases {
			let error = try await useError(json: json, exitCode: exitCode)
			XCTAssertEqual(error, .invalidResponse)
		}
	}

	func testUseRejectsAWrongEchoedKeyForEveryNonSuccessDispatchState() async throws {
		let wrongKey = "33333333-3333-4333-8333-333333333333"
		let cases: [(String, Int32)] = [
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"\(wrongKey)","dispatch_state":"definitely_not_dispatched","failure":"configuration_missing"}
				""",
				2
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"\(wrongKey)","dispatch_state":"potentially_dispatched","failure":"protocol_timeout"}
				""",
				2
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"rejected","idempotency_key":"\(wrongKey)","dispatch_state":"rejected_before_acceptance","error":{"reason":"idempotency_conflict"}}
				""",
				1
			),
		]
		for (json, exitCode) in cases {
			let error = try await useError(json: json, exitCode: exitCode)
			XCTAssertEqual(error, .invalidResponse)
		}
	}

	func testServiceUnavailableResponseDecodesAsTypedError() async throws {
		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"accounts","outcome":"unavailable","authority":{"profile_name":"local","server_id":"\(serverID)"},"result":{"outcome":"unavailable","data":{"error":"vault_unavailable"}}}'
			exit 1
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		do {
			_ = try await client.accounts()
			XCTFail("Typed service unavailability must fail.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .service(.vaultUnavailable))
		}
	}

	private func makeFixture(
		schema: String = "decodex/reset-card-cli/1"
	) throws -> ResetCardCLIFixture {
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
		[ -z "${RESET_CARD_TEST_SECRET+x}" ] || exit 90
		for argument in "$@"; do
			printf '%s\\n' "$argument" >> '\(logURL.path)'
		done
		printf '%s\\n' -- >> '\(logURL.path)'
		case "$*" in
			"--output json reset-card accounts")
				printf '%s\\n' '{"schema":"\(schema)","command":"accounts","outcome":"available","authority":{"profile_name":"local","server_id":"\(serverID)"},"result":{"outcome":"available","data":{"accounts":[{"account_id":"\(accountID)","display_label":"Account A","account_revision":7,"admission_state":"depleted"}]}}}'
				;;
			"--profile local --expected-server-id \(serverID) --output json reset-card list --account \(accountID)")
				printf '%s\\n' '{"schema":"\(schema)","command":"list","outcome":"available","authority":{"profile_name":"local","server_id":"\(serverID)"},"result":{"outcome":"available","data":{"account_id":"\(accountID)","account_revision":7,"available_count":1,"cards":[{"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}]}}}'
				;;
			"--profile local --expected-server-id \(serverID) --output json reset-card use --account \(accountID) --granted-at 100 --expires-at 200 --expected-revision 7 --idempotency-key \(idempotencyKey) --yes")
				printf '%s\\n' '{"schema":"\(schema)","command":"use","outcome":"completed","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"completed","data":{"outcome":"reset"}}}'
				;;
			"--profile local --expected-server-id \(serverID) --output json reset-card status --idempotency-key \(idempotencyKey)")
				printf '%s\\n' '{"schema":"\(schema)","command":"status","outcome":"completed","idempotency_key":"\(idempotencyKey)","state":{"state":"completed","data":{"outcome":"reset"}}}'
				;;
			*)
				exit 91
				;;
		esac
		"""
		try script.write(to: executableURL, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executableURL.path
		)

		return ResetCardCLIFixture(
			directoryURL: directoryURL,
			executableURL: executableURL,
			logURL: logURL
		)
	}

	private func makeRawFixture(body: String) throws -> ResetCardCLIFixture {
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
		\(body)
		"""
		try script.write(to: executableURL, atomically: true, encoding: .utf8)
		try FileManager.default.setAttributes(
			[.posixPermissions: 0o700],
			ofItemAtPath: executableURL.path
		)

		return ResetCardCLIFixture(
			directoryURL: directoryURL,
			executableURL: executableURL,
			logURL: logURL
		)
	}

	private func makeAttempt() throws -> ResetCardUseAttempt {
		ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: authority,
				accountID: accountID,
				expectedRevision: 7,
				descriptor: try ResetCardDescriptor(
					grantedAtUnixSeconds: 100,
					expiresAtUnixSeconds: 200
				)
			),
			idempotencyKey: idempotencyKey
		)
	}

	private func useError(
		json: String,
		exitCode: Int32
	) async throws -> ResetCardClientError {
		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '\(json)'
			exit \(exitCode)
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		do {
			_ = try await client.use(makeAttempt())
			XCTFail("The use document must not produce an operation state.")
			return .invalidResponse
		} catch let error as ResetCardClientError {
			return error
		}
	}
}

private struct ResetCardCLIFixture {
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
