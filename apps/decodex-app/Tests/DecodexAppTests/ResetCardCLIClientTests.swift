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

	func testAccountListArgumentsCanPinTheEstablishedAuthority() {
		XCTAssertEqual(
			ResetCardCLIClient.accountsArguments(authority: authority),
			[
				"--profile",
				"local",
				"--expected-server-id",
				serverID,
				"--output",
				"json",
				"account",
				"list",
			]
		)
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
					authority: nil,
					accountID: accountID,
					displayLabel: "Account A",
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
						state: .stale(
							usedPercent: 80,
							resetsAtUnixMicros: 3_000_000
						)
					)
				),
			]
		)

		let inventory = try await client.inventory(for: accounts[0])
		XCTAssertEqual(inventory.authority, authority)
		XCTAssertEqual(inventory.accountID, accountID)
		XCTAssertEqual(inventory.accountRevision, 7)
		XCTAssertEqual(inventory.fiveHourQuota.stateLabel, "Current")
		XCTAssertEqual(inventory.fiveHourQuota.usedPercent, 55)
		XCTAssertEqual(inventory.sevenDayQuota.stateLabel, "Stale")
		XCTAssertEqual(inventory.sevenDayQuota.usedPercent, 90)
		XCTAssertNil(inventory.observationError)
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
				["--output", "json", "account", "list"],
				[
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
		let fixture = try makeFixture(accountSchema: "wrong/schema")
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

	func testAccountListUsesRoutingOrderInsteadOfVectorOrder() async throws {
		let secondAccountID = "33333333-3333-4333-8333-333333333333"
		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/cli-account/1","command":"list","outcome":"success","result":{"outcome":"available","data":{"accounts":[{"account_id":"\(accountID)","display_label":"Account A","enabled":true,"account_revision":7,"observed_state":"available","lifecycle_readiness":"ready","credential_binding":{"schema_version":1,"version":1,"fingerprint_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","provider":"chatgpt","provider_account_id":"provider-a"},"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}},{"account_id":"\(secondAccountID)","display_label":"Account B","enabled":true,"account_revision":8,"observed_state":"depleted","lifecycle_readiness":"ready","credential_binding":{"schema_version":1,"version":1,"fingerprint_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","provider":"chatgpt","provider_account_id":"provider-b"},"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}],"routing":{"revision":3,"mode":{"mode":"fixed","account_id":"\(secondAccountID)"},"order":["\(secondAccountID)","\(accountID)"]}}}}'
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		let accounts = try await client.accounts()

		XCTAssertEqual(accounts.map(\.accountID), [secondAccountID, accountID])
		XCTAssertEqual(accounts.map(\.displayLabel), ["Account B", "Account A"])
	}

	func testAccountListRejectsUnknownFieldsAndAnUnroutableFixedTarget() async throws {
		let documents = [
			"""
			{"schema":"decodex/cli-account/1","command":"list","outcome":"success","result":{"outcome":"available","data":{"accounts":[{"account_id":"\(accountID)","display_label":"Account A","enabled":true,"account_revision":7,"observed_state":"available","lifecycle_readiness":"ready","unexpected":true,"credential_binding":{"schema_version":1,"version":1,"fingerprint_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","provider":"chatgpt","provider_account_id":"provider-a"},"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}],"routing":{"revision":3,"mode":{"mode":"balanced"},"order":["\(accountID)"]}}}}
			""",
			"""
			{"schema":"decodex/cli-account/1","command":"list","outcome":"success","result":{"outcome":"available","data":{"accounts":[{"account_id":"\(accountID)","display_label":"Account A","enabled":true,"account_revision":7,"observed_state":"available","lifecycle_readiness":"ready","credential_binding":{"schema_version":1,"version":1,"fingerprint_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","provider":"chatgpt","provider_account_id":"provider-a"},"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}],"routing":{"revision":3,"mode":{"mode":"fixed","account_id":"33333333-3333-4333-8333-333333333333"},"order":["\(accountID)"]}}}}
			""",
		]

		for document in documents {
			let fixture = try makeRawFixture(
				body: "printf '%s\\n' '\(document)'"
			)
			defer { fixture.remove() }
			let client = ResetCardCLIClient(
				executableURL: fixture.executableURL,
				environment: ["HOME": fixture.directoryURL.path],
				timeout: 2
			)

			do {
				_ = try await client.accounts()
				XCTFail("Incomplete routing or unknown account fields must fail closed.")
			} catch let error as ResetCardClientError {
				XCTAssertEqual(error, .invalidResponse)
			}
		}
	}

	func testObservationFailureRetainsTypedQuotaStatesPerAccount() async throws {
		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"list","outcome":"observation_failed","authority":{"profile_name":"local","server_id":"\(serverID)"},"result":{"outcome":"observation_failed","data":{"account_id":"\(accountID)","account_revision":7,"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":4000000,"result":{"state":"error","data":{"error":"provider_unavailable"}}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}},"error":"provider_unavailable"}}}'
			exit 1
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		let inventory = try await client.inventory(for: accountRecord())

		XCTAssertEqual(inventory.observationError, .providerUnavailable)
		XCTAssertEqual(inventory.fiveHourQuota.state, .error(.providerUnavailable))
		XCTAssertEqual(inventory.sevenDayQuota.state, .unknown)
		XCTAssertTrue(inventory.cards.isEmpty)
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

	func testUseAcceptsEveryCurrentLocalTransportFailureAndAccountRejection() async throws {
		for failure in [
			"local_transport_disabled",
			"remote_transport_disabled",
			"local_transport_unsupported",
			"unsafe_local_endpoint",
			"local_peer_identity_unavailable",
			"local_peer_uid_mismatch",
		] {
			let error = try await useError(
				json: """
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"\(idempotencyKey)","dispatch_state":"definitely_not_dispatched","failure":"\(failure)"}
				""",
				exitCode: 2
			)
			XCTAssertEqual(error, .useDefinitelyNotDispatched)
		}

		for errorDocument in [
			#"{"reason":"acceptance_unknown"}"#,
			#"{"reason":"account_command_rejected","rejection":"stale_account","actual_revision":7}"#,
		] {
			let error = try await useError(
				json: """
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"rejected","idempotency_key":"\(idempotencyKey)","dispatch_state":"rejected_before_acceptance","error":\(errorDocument)}
				""",
				exitCode: 1
			)
			XCTAssertEqual(error, .commandRejected)
		}
	}

	func testUseAndStatusRejectUnknownFieldsAtEveryOperationBoundary() async throws {
		let invalidUseDocuments: [(json: String, exitCode: Int32)] = [
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"completed","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"completed","data":{"outcome":"reset"}},"unexpected":true}
				""",
				0
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"completed","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"completed","data":{"outcome":"reset","unexpected":true}}}
				""",
				0
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"completed","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"completed","data":{"outcome":"reset"}},"error":null}
				""",
				0
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"prepared","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"prepared","data":null}}
				""",
				1
			),
			(
				"""
				{"schema":"decodex/reset-card-cli/1","command":"use","outcome":"failure","idempotency_key":"\(idempotencyKey)","dispatch_state":"definitely_not_dispatched","failure":"protocol_timeout","state":null}
				""",
				2
			),
		]
		for invalidUseDocument in invalidUseDocuments {
			let error = try await useError(
				json: invalidUseDocument.json,
				exitCode: invalidUseDocument.exitCode
			)
			XCTAssertEqual(error, .invalidResponse)
		}

		let fixture = try makeRawFixture(
			body: """
			printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"status","outcome":"completed","idempotency_key":"\(idempotencyKey)","state":{"state":"completed","data":{"outcome":"reset"}},"unexpected":true}'
			"""
		)
		defer { fixture.remove() }
		let client = ResetCardCLIClient(
			executableURL: fixture.executableURL,
			environment: ["HOME": fixture.directoryURL.path],
			timeout: 2
		)

		do {
			_ = try await client.status(for: makeAttempt())
			XCTFail("Unknown status fields must fail closed.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .invalidResponse)
		}
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
			printf '%s\\n' '{"schema":"decodex/reset-card-cli/1","command":"list","outcome":"unavailable","authority":{"profile_name":"local","server_id":"\(serverID)"},"result":{"outcome":"unavailable","data":{"error":"vault_unavailable"}}}'
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
			_ = try await client.inventory(for: accountRecord())
			XCTFail("Typed service unavailability must fail.")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .service(.vaultUnavailable))
		}
	}

	private func makeFixture(
		accountSchema: String = "decodex/cli-account/1",
		resetSchema: String = "decodex/reset-card-cli/1"
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
			"--output json account list")
				printf '%s\\n' '{"schema":"\(accountSchema)","command":"list","outcome":"success","result":{"outcome":"available","data":{"accounts":[{"account_id":"\(accountID)","display_label":"Account A","enabled":true,"account_revision":7,"observed_state":"depleted","lifecycle_readiness":"ready","credential_binding":{"schema_version":1,"version":1,"fingerprint_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","provider":"chatgpt","provider_account_id":"provider-a"},"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":1000000,"result":{"state":"current","data":{"used_percent":100,"resets_at_unix_micros":2000000}}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":1000000,"result":{"state":"stale","data":{"used_percent":80,"resets_at_unix_micros":3000000}}}}],"routing":{"revision":3,"mode":{"mode":"balanced"},"order":["\(accountID)"]}}}}'
				;;
			"--output json reset-card list --account \(accountID)")
				printf '%s\\n' '{"schema":"\(resetSchema)","command":"list","outcome":"available","authority":{"profile_name":"local","server_id":"\(serverID)"},"result":{"outcome":"available","data":{"account_id":"\(accountID)","account_revision":7,"available_count":1,"cards":[{"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}],"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":4000000,"result":{"state":"current","data":{"used_percent":55,"resets_at_unix_micros":5000000}}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":4000000,"result":{"state":"stale","data":{"used_percent":90,"resets_at_unix_micros":6000000}}}}}}'
				;;
			"--profile local --expected-server-id \(serverID) --output json reset-card use --account \(accountID) --granted-at 100 --expires-at 200 --expected-revision 7 --idempotency-key \(idempotencyKey) --yes")
				printf '%s\\n' '{"schema":"\(resetSchema)","command":"use","outcome":"completed","idempotency_key":"\(idempotencyKey)","dispatch_state":"durably_accepted","account_id":"\(accountID)","descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},"account_revision":7,"state":{"state":"completed","data":{"outcome":"reset"}}}'
				;;
			"--profile local --expected-server-id \(serverID) --output json reset-card status --idempotency-key \(idempotencyKey)")
				printf '%s\\n' '{"schema":"\(resetSchema)","command":"status","outcome":"completed","idempotency_key":"\(idempotencyKey)","state":{"state":"completed","data":{"outcome":"reset"}}}'
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

	private func accountRecord() -> ResetCardAccountRecord {
		ResetCardAccountRecord(
			authority: nil,
			accountID: accountID,
			displayLabel: "Account A",
			accountRevision: 7,
			enabled: true,
			observedState: .unknown,
			lifecycleReadiness: .ready,
			fiveHourQuota: .unknown(durationMinutes: 300),
			sevenDayQuota: .unknown(durationMinutes: 10_080)
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
