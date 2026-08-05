@testable import DecodexApp
import Foundation
import XCTest

final class ResetCardNativeClientTests: XCTestCase {
	private let accountID = "11111111-1111-4111-8111-111111111111"
	private let secondAccountID = "33333333-3333-4333-8333-333333333333"
	private let idempotencyKey = "22222222-2222-4222-8222-222222222222"
	private let serverID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"

	private var authority: ResetCardAuthority {
		ResetCardAuthority(profileName: "local", serverID: serverID)
	}

	func testNativeListBindsReturnedAuthorityAndUsesRoutingOrder() async throws {
		let accountID = accountID
		let secondAccountID = secondAccountID
		let authority = authority
		let recorder = NativeRequestRecorder()
		let accountsData = """
		{
		  "outcome":"available",
		  "data":{
		    "accounts":[
		      \(nativeAccountJSON(accountID: accountID, alias: "Iris", revision: 7)),
		      \(nativeAccountJSON(accountID: secondAccountID, alias: "Jamie", revision: 8))
		    ],
		    "routing":{
		      "revision":3,
		      "mode":{"mode":"fixed","account_id":"\(secondAccountID)"},
		      "order":["\(secondAccountID)","\(accountID)"]
		    }
		  }
		}
		"""
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "list_accounts",
				authority: authority,
				data: accountsData
			)
		}

		let snapshot = try await client.accountSnapshot(authority: nil)

		XCTAssertEqual(snapshot.authority, authority)
		XCTAssertEqual(snapshot.accounts.map(\.accountID), [secondAccountID, accountID])
		XCTAssertEqual(snapshot.accounts.map(\.authority), [authority, authority])
		XCTAssertEqual(
			snapshot.routing.mode,
			.fixed(accountID: secondAccountID)
		)
		let request = try XCTUnwrap(recorder.requests.first)
		XCTAssertNil(request.authority)
		XCTAssertEqual(
			try nativeJSONObject(request.data),
			[
				"schema": decodexNativeClientSchema,
				"operation": "list_accounts",
			]
		)
	}

	func testNativeObservationWaitUsesOneOpaqueDaemonGeneration() async throws {
		let authority = authority
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "wait_for_account_observation",
				authority: authority,
				data: #"{"generation":42}"#
			)
		}

		let signal = try await client.waitForAccountObservation(afterGeneration: 17)

		XCTAssertEqual(signal, AccountObservationSignal(generation: 42))
		let request = try XCTUnwrap(recorder.requests.first)
		XCTAssertNil(request.authority)
		XCTAssertEqual(
			try nativeJSONObject(request.data),
			[
				"schema": decodexNativeClientSchema,
				"operation": "wait_for_account_observation",
				"after_generation": 17,
			]
		)
	}

	func testNativePriorityObservationWaitRequestsOneCoalescedRefresh() async throws {
		let authority = authority
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "wait_for_account_observation",
				authority: authority,
				data: #"{"generation":43}"#
			)
		}

		let signal = try await client.requestAccountObservationRefresh(afterGeneration: 17)

		XCTAssertEqual(signal, AccountObservationSignal(generation: 43))
		let request = try XCTUnwrap(recorder.requests.first)
		XCTAssertNil(request.authority)
		XCTAssertEqual(
			try nativeJSONObject(request.data),
			[
				"schema": decodexNativeClientSchema,
				"operation": "wait_for_account_observation",
				"after_generation": 17,
				"request_refresh": true,
			]
		)
	}

	func testNativeInventoryAndStatusUsePinnedAuthority() async throws {
		let accountID = accountID
		let idempotencyKey = idempotencyKey
		let authority = authority
		let descriptor = try ResetCardDescriptor(
			grantedAtUnixSeconds: 100,
			expiresAtUnixSeconds: 200
		)
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			let object = try nativeJSONObject(request)
			switch object["operation"] as? String {
			case "get_reset_cards":
				return nativeSuccess(
					operation: "get_reset_cards",
					authority: authority,
					data: """
					{"outcome":"available","data":{
					  "account_id":"\(accountID)",
					  "account_revision":7,
					  "reported_available_count":1,
					  "details_complete":true,
					  "cards":[{"descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200}}],
					  "five_hour_quota":\(nativeQuotaJSON(duration: 300, used: 55, reset: 2_000_000)),
					  "seven_day_quota":\(nativeQuotaJSON(duration: 10080, used: 90, reset: 3_000_000))
					}}
					"""
				)
			case "reset_card_status":
				return nativeSuccess(
					operation: "reset_card_status",
					authority: authority,
					data: #"{"state":"completed","data":{"outcome":"reset"}}"#
				)
			default:
				throw ResetCardClientError.invalidResponse
			}
		}
		let account = nativeAccount(
			authority: authority,
			accountID: accountID,
			revision: 7
		)

		let inventory = try await client.inventory(for: account)
		XCTAssertEqual(inventory.cards, [descriptor])
		XCTAssertEqual(inventory.fiveHourQuota.usedPercent, 55)
		XCTAssertEqual(inventory.sevenDayQuota.usedPercent, 90)

		let attempt = ResetCardUseAttempt(
			target: ResetCardUseTarget(
				authority: authority,
				accountID: accountID,
				expectedRevision: 7,
				descriptor: descriptor
			),
			idempotencyKey: idempotencyKey
		)
		let state = try await client.status(for: attempt)
		XCTAssertEqual(state, .completed(.reset))
		XCTAssertEqual(recorder.requests.count, 2)
		XCTAssertEqual(recorder.requests.map(\.authority), [authority, authority])
		XCTAssertEqual(
			try nativeJSONObject(recorder.requests[0].data)["account_id"] as? String,
			accountID
		)
		XCTAssertEqual(
			try nativeJSONObject(recorder.requests[1].data)["idempotency_key"] as? String,
			idempotencyKey
		)
	}

	func testNativeInventoryAcceptsUnsupportedFiveHourWindowFromLiveDaemon() async throws {
		let accountID = "b7639aa9-ccc1-4957-8bd8-9a54ee909c43"
		let authority = ResetCardAuthority(
			profileName: "local",
			serverID: "0939ea28-c79b-40d5-b78f-b0c4c6790c17"
		)
		let client = DecodexNativeClient { _, _ in
			Data(
				"""
				{"schema":"decodex/app-native-client/1","outcome":"success","operation":"get_reset_cards","authority":{"profile_name":"local","server_id":"0939ea28-c79b-40d5-b78f-b0c4c6790c17"},"data":{"data":{"account_id":"b7639aa9-ccc1-4957-8bd8-9a54ee909c43","account_revision":8,"reported_available_count":2,"details_complete":true,"cards":[{"descriptor":{"expires_at_unix_seconds":1785528152,"granted_at_unix_seconds":1782936152}},{"descriptor":{"expires_at_unix_seconds":1786556624,"granted_at_unix_seconds":1783964624}}],"five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":1785335237831965,"result":{"data":{"error":"unsupported_window"},"state":"error"}},"seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":1785335237831965,"result":{"data":{"resets_at_unix_micros":1785940048000000,"used_percent":0},"state":"current"}}},"outcome":"available"}}
				""".utf8
			)
		}

		let inventory = try await client.inventory(
			for: nativeAccount(authority: authority, accountID: accountID, revision: 8)
		)

		XCTAssertEqual(inventory.cards.count, 2)
		XCTAssertEqual(inventory.fiveHourQuota.state, .error(.unsupportedWindow))
		XCTAssertEqual(inventory.sevenDayQuota.usedPercent, 0)
	}

	func testNativeInventoryRejectsRetiredStaleQuotaState() async {
		let accountID = accountID
		let authority = authority
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "get_reset_cards",
				authority: authority,
				data: """
				{"outcome":"available","data":{
				  "account_id":"\(accountID)",
				  "account_revision":7,
				  "reported_available_count":0,
				  "details_complete":true,
				  "cards":[],
				  "five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":1000000,"result":{"state":"stale","data":{"used_percent":20,"resets_at_unix_micros":2000000}}},
				  "seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
				}}
				"""
			)
		}

		do {
			_ = try await client.inventory(
				for: nativeAccount(authority: authority, accountID: accountID, revision: 7)
			)
			XCTFail("retired stale quota state must be rejected")
		} catch {
			XCTAssertEqual(error as? ResetCardClientError, .invalidResponse)
		}
	}

	func testNativeInventoryAcceptsCountOnlyPartialDetailsWithoutSelectableCards() async throws {
		let accountID = accountID
		let authority = authority
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "get_reset_cards",
				authority: authority,
				data: """
				{"outcome":"available","data":{
				  "account_id":"\(accountID)",
				  "account_revision":7,
				  "reported_available_count":2,
				  "details_complete":false,
				  "cards":[],
				  "five_hour_quota":\(nativeQuotaJSON(duration: 300, used: 55, reset: 2_000_000)),
				  "seven_day_quota":\(nativeQuotaJSON(duration: 10080, used: 90, reset: 3_000_000))
				}}
				"""
			)
		}

		let inventory = try await client.inventory(
			for: nativeAccount(authority: authority, accountID: accountID, revision: 7)
		)
		let state = ResetCardAccountState(
			account: nativeAccount(authority: authority, accountID: accountID, revision: 7),
			inventory: inventory,
			error: nil,
			isRefreshing: false
		)

		XCTAssertEqual(inventory.reportedAvailableCount, 2)
		XCTAssertFalse(inventory.detailsComplete)
		XCTAssertTrue(inventory.cards.isEmpty)
		XCTAssertTrue(state.targets.isEmpty)
		XCTAssertEqual(inventory.fiveHourQuota.usedPercent, 55)
		XCTAssertEqual(inventory.sevenDayQuota.usedPercent, 90)
	}

	func testNativeInventoryMapsTheDaemonDeadlineToATypedRetryableServiceError() async {
		let accountID = accountID
		let authority = authority
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "get_reset_cards",
				authority: authority,
				data: #"{"outcome":"unavailable","data":{"error":"request_timed_out"}}"#
			)
		}

		do {
			_ = try await client.inventory(
				for: nativeAccount(authority: authority, accountID: accountID, revision: 7)
			)
			XCTFail("Expected a typed deadline failure")
		} catch {
			XCTAssertEqual(
				error as? ResetCardClientError,
				.service(.requestTimedOut)
			)
		}
	}

	func testNativeInventoryRejectsAZeroCountMarkedAsPartial() async {
		let accountID = accountID
		let authority = authority
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "get_reset_cards",
				authority: authority,
				data: """
				{"outcome":"available","data":{
				  "account_id":"\(accountID)",
				  "account_revision":7,
				  "reported_available_count":0,
				  "details_complete":false,
				  "cards":[],
				  "five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				  "seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}
				}}
				"""
			)
		}

		do {
			_ = try await client.inventory(
				for: nativeAccount(authority: authority, accountID: accountID, revision: 7)
			)
			XCTFail("zero reported count must be a complete empty inventory")
		} catch {
			XCTAssertEqual(error as? ResetCardClientError, .invalidResponse)
		}
	}

	func testObservationFailureRetainsTypedQuotaStates() async throws {
		let accountID = accountID
		let authority = authority
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "get_reset_cards",
				authority: authority,
				data: """
				{"outcome":"observation_failed","data":{
				  "account_id":"\(accountID)",
				  "account_revision":7,
				  "five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
				  "seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":1000000,"result":{"state":"error","data":{"error":"provider_unavailable"}}},
				  "error":"inventory_incomplete"
				}}
				"""
			)
		}

		let value = try await client.inventory(
			for: nativeAccount(authority: authority, accountID: accountID, revision: 7)
		)
		XCTAssertEqual(value.cards, [])
		XCTAssertEqual(value.observationError, .inventoryIncomplete)
		XCTAssertEqual(value.fiveHourQuota.state, .unknown)
		XCTAssertEqual(value.sevenDayQuota.state, .error(.providerUnavailable))
	}

	func testUseAcceptedValidatesExactTargetAndReturnsState() async throws {
		let accountID = accountID
		let authority = authority
		let idempotencyKey = idempotencyKey
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "use_reset_card",
				authority: authority,
				data: """
				{"outcome":"accepted","data":{
				  "account_id":"\(accountID)",
				  "descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},
				  "state":{"state":"completed","data":{"outcome":"reset"}},
				  "entity_revision":7
				}}
				"""
			)
		}
		let attempt = try nativeAttempt(
			authority: authority,
			accountID: accountID,
			revision: 7,
			idempotencyKey: idempotencyKey
		)

		let state = try await client.use(attempt)
		XCTAssertEqual(state, .completed(.reset))
		let request = try nativeJSONObject(XCTUnwrap(recorder.requests.first).data)
		XCTAssertEqual(request["operation"] as? String, "use_reset_card")
		XCTAssertEqual(request["account_id"] as? String, accountID)
		XCTAssertEqual(request["expected_revision"] as? NSNumber, 7)
		XCTAssertEqual(request["idempotency_key"] as? String, idempotencyKey)
		XCTAssertEqual(Set(request.keys), [
			"schema", "operation", "account_id", "granted_at_unix_seconds",
			"expires_at_unix_seconds", "expected_revision", "idempotency_key",
		])
	}

	func testUseRejectsRevisionDriftAndTypedNonacceptance() async throws {
		let accountID = accountID
		let authority = authority
		let attempt = try nativeAttempt(
			authority: authority,
			accountID: accountID,
			revision: 7,
			idempotencyKey: idempotencyKey
		)
		let responses = [
			"""
			{"outcome":"accepted","data":{
			  "account_id":"\(accountID)",
			  "descriptor":{"granted_at_unix_seconds":100,"expires_at_unix_seconds":200},
			  "state":{"state":"prepared"},
			  "entity_revision":8
			}}
			""",
			#"{"outcome":"rejected","data":{"error":{"reason":"idempotency_conflict"}}}"#,
			#"{"outcome":"potentially_dispatched","data":{"failure":"protocol_timeout"}}"#,
		]
		let expected: [ResetCardClientError] = [
			.invalidResponse,
			.commandRejected,
			.usePotentiallyDispatched,
		]
		for (data, expectedError) in zip(responses, expected) {
			let client = DecodexNativeClient { _, _ in
				nativeSuccess(
					operation: "use_reset_card",
					authority: authority,
					data: data
				)
			}
			do {
				_ = try await client.use(attempt)
				XCTFail("Expected typed failure")
			} catch let error as ResetCardClientError {
				XCTAssertEqual(error, expectedError)
			}
		}
	}

	func testEnvelopeRejectsWrongSchemaOperationUnknownFieldsAndAuthorityDrift() async throws {
		let authority = authority
		let alternate = ResetCardAuthority(
			profileName: "other",
			serverID: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
		)
		let payload = #"{"outcome":"unavailable"}"#
		let malformed = [
			"""
			{"schema":"wrong","outcome":"success","operation":"list_accounts",
			 "authority":{"profile_name":"local","server_id":"\(serverID)"},"data":\(payload)}
			""",
			"""
			{"schema":"\(decodexNativeClientSchema)","outcome":"success","operation":"get_reset_cards",
			 "authority":{"profile_name":"local","server_id":"\(serverID)"},"data":\(payload)}
			""",
			"""
			{"schema":"\(decodexNativeClientSchema)","outcome":"success","operation":"list_accounts",
			 "authority":{"profile_name":"local","server_id":"\(serverID)","extra":true},"data":\(payload)}
			""",
			"""
			{"schema":"\(decodexNativeClientSchema)","outcome":"success","operation":"list_accounts",
			 "authority":{"profile_name":"\(alternate.profileName)","server_id":"\(alternate.serverID)"},"data":\(payload)}
			""",
		]
		for document in malformed {
			let client = DecodexNativeClient { _, _ in Data(document.utf8) }
			do {
				_ = try await client.accounts(authority: authority)
				XCTFail("Malformed native envelope must fail")
			} catch let error as ResetCardClientError {
				XCTAssertEqual(error, .invalidResponse)
				XCTAssertEqual(String(reflecting: error), "ResetCardClientError.invalidResponse")
			}
		}
	}

	func testClosedOuterFailuresRemainTyped() async throws {
		let authority = authority
		let cases: [(String, ResetCardClientError)] = [
			("protocol_timeout", .timedOut),
			("protocol_disconnected", .transportDisconnected),
			("protocol_backpressure", .transportBackpressured),
			("runtime_unavailable", .nativeClientUnavailable),
			("protocol_malformed", .invalidResponse),
		]
		for (failure, expected) in cases {
			let client = DecodexNativeClient { _, _ in
				nativeFailure(operation: "list_accounts", failure: failure)
			}
			do {
				_ = try await client.accounts(authority: authority)
				XCTFail("Native failure must remain typed")
			} catch let error as ResetCardClientError {
				XCTAssertEqual(error, expected)
			}
		}
	}

	func testResponseSizeIsBoundedBeforeDecoding() async throws {
		let client = DecodexNativeClient { _, _ in
			Data(repeating: 0x20, count: 8 * 1024 * 1024 + 1)
		}
		do {
			_ = try await client.accounts()
			XCTFail("Oversized native response must fail")
		} catch let error as ResetCardClientError {
			XCTAssertEqual(error, .outputTooLarge)
		}
	}

	func testAccountListRejectsUnknownFieldsAndUnroutableFixedTarget() async throws {
		let accountID = accountID
		let authority = authority
		let documents = [
			"""
			{"outcome":"available","data":{"accounts":[
			  {"account_id":"\(accountID)","alias":"Iris","enabled":true,
			   "account_revision":7,"observed_state":"available","lifecycle_readiness":"ready",
			   "unexpected":true,
			   "credential_binding":{"schema_version":1,"version":1,"fingerprint_sha256":"\(String(repeating: "a", count: 64))","provider":"chatgpt","provider_account_id":"provider-a"},
			   "five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
			   "seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}
			],"routing":{"revision":3,"mode":{"mode":"balanced"},"order":["\(accountID)"]}}}
			""",
			"""
			{"outcome":"available","data":{"accounts":[
			  \(nativeAccountJSON(accountID: accountID, alias: "Iris", revision: 7))
			],"routing":{"revision":3,"mode":{"mode":"fixed","account_id":"\(secondAccountID)"},"order":["\(accountID)"]}}}
			""",
			"""
			{"outcome":"available","data":{"accounts":[
			  \(nativeAccountJSON(accountID: accountID, alias: "Account 00000-00001", revision: 7))
			],"routing":{"revision":3,"mode":{"mode":"balanced"},"order":["\(accountID)"]}}}
			""",
		]
		for data in documents {
			let client = DecodexNativeClient { _, _ in
				nativeSuccess(operation: "list_accounts", authority: authority, data: data)
			}
			await XCTAssertThrowsInvalidResponse {
				_ = try await client.accounts()
			}
		}
	}
}

struct RecordedNativeRequest: @unchecked Sendable {
	let data: Data
	let authority: ResetCardAuthority?
}

final class NativeRequestRecorder: @unchecked Sendable {
	private let lock = NSLock()
	private var storage: [RecordedNativeRequest] = []

	func append(_ data: Data, authority: ResetCardAuthority?) {
		lock.withLock {
			storage.append(RecordedNativeRequest(data: data, authority: authority))
		}
	}

	var requests: [RecordedNativeRequest] {
		lock.withLock { storage }
	}
}

func nativeSuccess(
	operation: String,
	authority: ResetCardAuthority,
	data: String
) -> Data {
	Data(
		"""
		{"schema":"\(decodexNativeClientSchema)","outcome":"success",
		 "operation":"\(operation)",
		 "authority":{"profile_name":"\(authority.profileName)","server_id":"\(authority.serverID)"},
		 "data":\(data)}
		""".utf8
	)
}

func nativeFailure(operation: String, failure: String) -> Data {
	Data(
		"""
		{"schema":"\(decodexNativeClientSchema)","outcome":"failure",
		 "operation":"\(operation)","failure":"\(failure)"}
		""".utf8
	)
}

func nativeJSONObject(_ data: Data) throws -> [String: AnyHashable] {
	let value = try JSONSerialization.jsonObject(with: data)
	guard let object = value as? [String: AnyHashable] else {
		throw ResetCardClientError.invalidResponse
	}
	return object
}

func nativeAccountJSON(
	accountID: String,
	alias: String,
	revision: UInt64
) -> String {
	"""
	{"account_id":"\(accountID)","alias":"\(alias)","enabled":true,
	 "account_revision":\(revision),"observed_state":"available","lifecycle_readiness":"ready",
	 "credential_binding":{"schema_version":1,"version":1,
	   "fingerprint_sha256":"\(String(repeating: "a", count: 64))",
	   "provider":"chatgpt","provider_account_id":"provider-\(revision)"},
	 "five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
	 "seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}
	"""
}

func nativeQuotaJSON(duration: UInt32, used: UInt8, reset: Int64) -> String {
	"""
	{"duration_minutes":\(duration),"observed_at_unix_micros":1000000,
	 "result":{"state":"current","data":{"used_percent":\(used),"resets_at_unix_micros":\(reset)}}}
	"""
}

func nativeAccount(
	authority: ResetCardAuthority,
	accountID: String,
	revision: UInt64
) -> ResetCardAccountRecord {
	ResetCardAccountRecord(
		authority: authority,
		accountID: accountID,
		alias: "Account 00000-00001",
		accountRevision: revision,
		enabled: true,
		observedState: .available,
		lifecycleReadiness: .ready,
		credentialBinding: AccountCredentialBinding(
			schemaVersion: 1,
			version: 1,
			fingerprintSHA256: String(repeating: "a", count: 64),
			provider: .chatGPT,
			providerAccountID: "provider"
		),
		fiveHourQuota: .unknown(durationMinutes: 300),
		sevenDayQuota: .unknown(durationMinutes: 10_080)
	)
}

func nativeAttempt(
	authority: ResetCardAuthority,
	accountID: String,
	revision: UInt64,
	idempotencyKey: String
) throws -> ResetCardUseAttempt {
	ResetCardUseAttempt(
		target: ResetCardUseTarget(
			authority: authority,
			accountID: accountID,
			expectedRevision: revision,
			descriptor: try ResetCardDescriptor(
				grantedAtUnixSeconds: 100,
				expiresAtUnixSeconds: 200
			)
		),
		idempotencyKey: idempotencyKey
	)
}

func XCTAssertThrowsInvalidResponse(
	_ operation: () async throws -> Void,
	file: StaticString = #filePath,
	line: UInt = #line
) async {
	do {
		try await operation()
		XCTFail("Expected invalid native response", file: file, line: line)
	} catch let error as ResetCardClientError {
		XCTAssertEqual(error, .invalidResponse, file: file, line: line)
	} catch {
		XCTFail("Unexpected error: \(error)", file: file, line: line)
	}
}
