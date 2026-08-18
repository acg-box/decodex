@testable import DecodexApp
import Foundation
import XCTest

final class AccountControlNativeClientTests: XCTestCase {
	private let accountID = "11111111-1111-4111-8111-111111111111"
	private let secondAccountID = "22222222-2222-4222-8222-222222222222"
	private let operationID = "33333333-3333-4333-8333-333333333333"
	private let idempotencyKey = "44444444-4444-4444-8444-444444444444"
	private let serverID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"

	private var authority: ResetCardAuthority {
		ResetCardAuthority(profileName: "local", serverID: serverID)
	}

	func testAccountSnapshotRetainsRoutingAndCredentialNegativeLifecycleFacts() async throws {
		let authority = authority
		let accountID = accountID
		let secondAccountID = secondAccountID
		let operationID = operationID
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "list_accounts",
				authority: authority,
				data: """
				{"outcome":"available","data":{
				  "accounts":[
				    \(controlAccountJSON(accountID: accountID, alias: "Iris", revision: 7)),
				    \(controlUnsettledAccountJSON(accountID: secondAccountID, operationID: operationID))
				  ],
				  "routing":{"revision":9,"mode":{"mode":"fixed","account_id":"\(secondAccountID)"},"order":["\(secondAccountID)","\(accountID)"]}
				}}
				"""
			)
		}

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
		XCTAssertEqual(snapshot.accounts[1].credentialBinding?.version, 3)
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

	func testEveryLifecycleAndRoutingCommandUsesExactNativeRequest() async throws {
		let authority = authority
		let accountID = accountID
		let secondAccountID = secondAccountID
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			let object = try nativeJSONObject(request)
			let operation = try XCTUnwrap(object["operation"] as? String)
			let enabled = (object["enabled"] as? Bool)
				?? (operation != "disable_account")
			let payload: String
			switch operation {
			case "get_codex_auth_projection":
				payload = """
				{"outcome":"current","data":{"account_id":"\(accountID)",
				  "account_revision":7,"projection_digest":"\(String(repeating: "c", count: 64))"}}
				"""
			case "use_account_in_codex":
				payload = """
				{"outcome":"applied","data":{"entity_revision":7,
				  "result":{"name":"codex_auth_projected","data":{
				    "account_id":"\(accountID)","account_revision":7,
				    "projection_digest":"\(String(repeating: "c", count: 64))"}}}}
				"""
			case "logout_account":
				payload = """
				{"outcome":"applied","data":{"entity_revision":8,
				  "result":{"name":"account_logged_out","data":{"account_id":"\(accountID)","tombstone_revision":8}}}}
				"""
			case "set_fixed_selection":
				payload = controlRoutingAppliedJSON(
					revision: 10,
					mode: #"{"mode":"fixed","account_id":"\#(accountID)"}"#,
					order: [accountID, secondAccountID]
				)
			case "set_balanced_selection":
				payload = controlRoutingAppliedJSON(
					revision: 10,
					mode: #"{"mode":"balanced"}"#,
					order: [accountID, secondAccountID]
				)
			case "set_account_order":
				payload = controlRoutingAppliedJSON(
					revision: 10,
					mode: #"{"mode":"balanced"}"#,
					order: [secondAccountID, accountID]
				)
			default:
				payload = """
				{"outcome":"applied","data":{"entity_revision":8,
				  "result":{"name":"account_changed","data":{"account":
				    \(controlAccountJSON(accountID: accountID, alias: "Iris", revision: 8, enabled: enabled))
				  }}}}
				"""
			}
			return nativeSuccess(
				operation: operation,
				authority: authority,
				data: payload
			)
		}

		_ = try await client.enrollFromSharedCodex(
			authority: authority,
			operationID: operationID,
			accountID: accountID,
			enabled: true,
			idempotencyKey: idempotencyKey
		)
		let projection = try await client.codexAuthProjection(authority: authority)
		XCTAssertEqual(
			projection,
			.current(
				accountID: accountID,
				accountRevision: 7,
				projectionDigest: String(repeating: "c", count: 64)
			)
		)
		_ = try await client.useAccountInCodex(
			authority: authority,
			accountID: accountID,
			expectedRevision: 7,
			idempotencyKey: idempotencyKey
		)
		_ = try await client.setAccountEnabled(
			authority: authority,
			accountID: accountID,
			enabled: false,
			expectedRevision: 7,
			idempotencyKey: idempotencyKey
		)
		_ = try await client.logoutAccount(
			authority: authority,
			operationID: operationID,
			accountID: accountID,
			expectedRevision: 7,
			idempotencyKey: idempotencyKey
		)
		_ = try await client.setFixedSelection(
			authority: authority,
			accountID: accountID,
			expectedAccountRevision: 7,
			expectedRoutingRevision: 9,
			idempotencyKey: idempotencyKey
		)
		_ = try await client.setBalancedSelection(
			authority: authority,
			expectedRoutingRevision: 9,
			idempotencyKey: idempotencyKey
		)
		_ = try await client.setAccountOrder(
			authority: authority,
			order: [secondAccountID, accountID],
			expectedRoutingRevision: 9,
			idempotencyKey: idempotencyKey
		)
		let requests = try recorder.requests.map { try nativeJSONObject($0.data) }
		XCTAssertEqual(
			requests.compactMap { $0["operation"] as? String },
			[
				"enroll_account", "get_codex_auth_projection", "use_account_in_codex",
				"disable_account",
				"logout_account", "set_fixed_selection",
				"set_balanced_selection", "set_account_order",
			]
		)
		XCTAssertEqual(
			Set(requests[0].keys),
			[
				"schema", "operation", "operation_id", "account_id",
				"enabled", "idempotency_key",
			]
		)
		XCTAssertEqual(Set(requests[1].keys), ["schema", "operation"])
		XCTAssertEqual(requests[2]["expected_revision"] as? NSNumber, 7)
		XCTAssertEqual(requests[3]["enabled"] as? Bool, nil)
		XCTAssertEqual(requests[3]["expected_revision"] as? NSNumber, 7)
		XCTAssertEqual(requests[4]["operation_id"] as? String, operationID)
		XCTAssertEqual(requests[5]["expected_account_revision"] as? NSNumber, 7)
		XCTAssertEqual(requests[5]["expected_routing_revision"] as? NSNumber, 9)
		XCTAssertEqual(Set(requests[6].keys), [
			"schema", "operation", "expected_routing_revision", "idempotency_key",
		])
		XCTAssertEqual(
			requests[7]["order"] as? [String],
			[secondAccountID, accountID]
		)
		XCTAssertEqual(Set(requests[7].keys), [
			"schema", "operation", "order", "expected_routing_revision", "idempotency_key",
		])
		XCTAssertEqual(recorder.requests.map(\.authority), Array(repeating: authority, count: 8))
	}

	func testAccountOrderRejectsAContradictoryAppliedOrder() async throws {
		let authority = authority
		let accountID = accountID
		let secondAccountID = secondAccountID
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "set_account_order",
				authority: authority,
				data: controlRoutingAppliedJSON(
					revision: 10,
					mode: #"{"mode":"balanced"}"#,
					order: [accountID, secondAccountID]
				)
			)
		}

		do {
			_ = try await client.setAccountOrder(
				authority: authority,
				order: [secondAccountID, accountID],
				expectedRoutingRevision: 9,
				idempotencyKey: idempotencyKey
			)
			XCTFail("A contradictory account order must not appear applied")
		} catch let error as AccountControlError {
			XCTAssertEqual(error, .invalidResponse)
		}
	}

	func testUseInCodexRejectsNoncanonicalIdentityRevisionAndAuthorityBeforeDispatch() async {
		let recorder = NativeRequestRecorder()
		let authority = authority
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			throw ResetCardClientError.invalidResponse
		}
		let invalidAuthority = ResetCardAuthority(
			profileName: "local space",
			serverID: serverID
		)
		let uppercaseAccountID = "abcdefab-cdef-4abc-8def-abcdefabcdef".uppercased()
		let inputs: [(ResetCardAuthority?, String, UInt64)] = [
			(authority, uppercaseAccountID, 1),
			(authority, accountID, 0),
			(invalidAuthority, accountID, 1),
		]
		for (authority, accountID, revision) in inputs {
			do {
				_ = try await client.useAccountInCodex(
					authority: authority,
					accountID: accountID,
					expectedRevision: revision,
					idempotencyKey: idempotencyKey
				)
				XCTFail("Invalid account command must fail")
			} catch let error as AccountControlError {
				XCTAssertEqual(error, .invalidInput)
			} catch {
				XCTFail("Unexpected error: \(error)")
			}
		}
		XCTAssertTrue(recorder.requests.isEmpty)
	}

	func testFixedSelectionDecodesStrictAppliedRoutingResult() async throws {
		let authority = authority
		let accountID = accountID
		let secondAccountID = secondAccountID
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "set_fixed_selection",
				authority: authority,
				data: controlRoutingAppliedJSON(
					revision: 10,
					mode: #"{"mode":"fixed","account_id":"\#(accountID)"}"#,
					order: [accountID, secondAccountID]
				)
			)
		}

		let result = try await client.setFixedSelection(
			authority: authority,
			accountID: accountID,
			expectedAccountRevision: 7,
			expectedRoutingRevision: 9,
			idempotencyKey: idempotencyKey
		)

		XCTAssertEqual(
			result,
			AccountControlResult.routingChanged(
				AccountRoutingControl(
					revision: 10,
					mode: .fixed(accountID: accountID),
					order: [accountID, secondAccountID]
				)
			)
		)
	}

	func testTypedRejectionRetainsCurrentOwningRevision() async throws {
		let authority = authority
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "set_balanced_selection",
				authority: authority,
				data: """
				{"outcome":"rejected","data":{"error":{
				  "reason":"account_command_rejected",
				  "rejection":"stale_routing_control","actual_revision":10
				}}}
				"""
			)
		}

		do {
			_ = try await client.setBalancedSelection(
				authority: authority,
				expectedRoutingRevision: 9,
				idempotencyKey: idempotencyKey
			)
			XCTFail("Rejected routing must not appear applied")
		} catch let error as AccountControlError {
			XCTAssertEqual(
				error,
				.rejected(.staleRoutingControl, actualRevision: 10)
			)
		}
	}

	func testSharedImportDuplicateUsesTypedEnrollmentRejectionAndExactRequest() async throws {
		let authority = authority
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "enroll_account",
				authority: authority,
				data: """
				{"outcome":"rejected","data":{"error":{
				  "reason":"account_command_rejected",
				  "rejection":"provider_already_enrolled"
				}}}
				"""
			)
		}

		do {
			_ = try await client.enrollFromSharedCodex(
				authority: authority,
				operationID: operationID,
				accountID: accountID,
				enabled: true,
				idempotencyKey: idempotencyKey
			)
			XCTFail("A duplicate shared login must not appear applied")
		} catch let error as AccountControlError {
			XCTAssertEqual(
				error,
				.rejected(.providerAlreadyEnrolled, actualRevision: nil)
			)
			XCTAssertEqual(
				error.localizedDescription,
				"This Codex login is already added. Choose a different account on the login page, then try again."
			)
		}

		let request = try XCTUnwrap(recorder.requests.first)
		let object = try nativeJSONObject(request.data)
		XCTAssertEqual(object["operation"] as? String, "enroll_account")
		XCTAssertEqual(object["operation_id"] as? String, operationID)
		XCTAssertEqual(object["account_id"] as? String, accountID)
		XCTAssertEqual(request.authority, authority)
	}

	func testAccountEnrollmentUsesExactNativeStartRequest() async throws {
		let authority = authority
		let sessionID = "55555555-5555-4555-8555-555555555555"
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "start_account_enrollment",
				authority: authority,
				data: """
					{"session_id":"\(sessionID)","state":"opening_browser"}
					"""
			)
		}

		let status = try await client.startAccountEnrollment(
			authority: authority,
			sessionID: sessionID,
			operationID: operationID,
			accountID: accountID,
			enabled: true,
			idempotencyKey: idempotencyKey,
			codexBin: "/Applications/ChatGPT.app/Contents/Resources/codex",
			loginMethod: .browserRedirect
		)

		XCTAssertEqual(status.state, .openingBrowser)
		let request = try XCTUnwrap(recorder.requests.first)
		let object = try nativeJSONObject(request.data)
		XCTAssertEqual(
			Set(object.keys),
			[
				"schema", "operation", "session_id", "operation_id",
				"account_id", "enabled", "idempotency_key", "codex_bin",
				"login_method",
			]
		)
		XCTAssertEqual(object["operation"] as? String, "start_account_enrollment")
		XCTAssertEqual(object["session_id"] as? String, sessionID)
		XCTAssertEqual(object["operation_id"] as? String, operationID)
		XCTAssertEqual(object["account_id"] as? String, accountID)
		XCTAssertEqual(object["enabled"] as? Bool, true)
		XCTAssertEqual(object["idempotency_key"] as? String, idempotencyKey)
		XCTAssertEqual(object["login_method"] as? String, "browser_redirect")
		XCTAssertEqual(
			object["codex_bin"] as? String,
			"/Applications/ChatGPT.app/Contents/Resources/codex"
		)
		XCTAssertEqual(request.authority, authority)
	}

	func testAccountCommandRejectsUnknownAndContradictoryAppliedResults() async throws {
		let authority = authority
		let accountID = accountID
		let documents = [
			"""
			{"outcome":"applied","data":{"entity_revision":10,
			  "result":{"name":"account_routing_changed","data":{"routing":{
			    "revision":10,"mode":{"mode":"balanced"},"order":["\(accountID)"],"unexpected":true
			  }}}}}
			""",
			"""
			{"outcome":"applied","data":{"entity_revision":10,
			  "result":{"name":"account_routing_changed","data":{"routing":{
			    "revision":10,"mode":{"mode":"fixed","account_id":"\(accountID)"},"order":["\(accountID)"]
			  }}}}}
			""",
		]
		for data in documents {
			let client = DecodexNativeClient { _, _ in
				nativeSuccess(
					operation: "set_balanced_selection",
					authority: authority,
					data: data
				)
			}
			do {
				_ = try await client.setBalancedSelection(
					authority: authority,
					expectedRoutingRevision: 9,
					idempotencyKey: idempotencyKey
				)
				XCTFail("Malformed account result must fail")
			} catch let error as AccountControlError {
				XCTAssertEqual(error, .invalidResponse)
			}
		}
	}

	func testAccountReauthenticationUsesExactStartPollAndCancelRequests() async throws {
		let authority = authority
		let sessionID = "55555555-5555-4555-8555-555555555555"
		let recoveryOperationID = "66666666-6666-4666-8666-666666666666"
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			let object = try nativeJSONObject(request)
			let operation = try XCTUnwrap(object["operation"] as? String)
			let data: String
			switch operation {
			case "start_account_reauthentication":
				data = """
					{"session_id":"\(sessionID)","state":"requesting_code"}
					"""
			case "poll_account_reauthentication":
				data = """
					{"session_id":"\(sessionID)","state":"waiting_for_browser",
					 "prompt":{"verification_url":"https://auth.openai.com/codex/device",
					 "user_code":"AB12-CDE34"}}
					"""
			case "cancel_account_reauthentication":
				data = """
					{"session_id":"\(sessionID)","state":"cancelled"}
					"""
			default:
				return nativeFailure(operation: operation, failure: "invalid_request")
			}
			return nativeSuccess(
				operation: operation,
				authority: authority,
				data: data
			)
		}

		let started = try await client.startAccountReauthentication(
			authority: authority,
			sessionID: sessionID,
			operationID: operationID,
			accountID: accountID,
			expectedRevision: 7,
			recoveryOperationID: recoveryOperationID,
			idempotencyKey: idempotencyKey,
			codexBin: "/Applications/ChatGPT.app/Contents/Resources/codex",
			loginMethod: .deviceCode
		)
		let polled = try await client.pollAccountReauthentication(
			authority: authority,
			sessionID: sessionID
		)
		let cancelled = try await client.cancelAccountReauthentication(
			authority: authority,
			sessionID: sessionID
		)

		XCTAssertEqual(started.state, .requestingCode)
		XCTAssertEqual(polled.state, .waitingForBrowser)
		XCTAssertEqual(polled.prompt?.userCode, "AB12-CDE34")
		XCTAssertEqual(cancelled.state, .cancelled)

		let requests = try recorder.requests.map { try nativeJSONObject($0.data) }
		XCTAssertEqual(
			requests.compactMap { $0["operation"] as? String },
			[
				"start_account_reauthentication",
				"poll_account_reauthentication",
				"cancel_account_reauthentication",
			]
		)
		XCTAssertEqual(
			Set(requests[0].keys),
			[
				"schema", "operation", "session_id", "operation_id",
				"account_id", "expected_revision", "recovery_operation_id",
				"idempotency_key", "codex_bin", "login_method",
			]
		)
		XCTAssertEqual(requests[0]["session_id"] as? String, sessionID)
		XCTAssertEqual(requests[0]["account_id"] as? String, accountID)
		XCTAssertEqual(requests[0]["expected_revision"] as? NSNumber, 7)
		XCTAssertEqual(
			requests[0]["recovery_operation_id"] as? String,
			recoveryOperationID
		)
		XCTAssertEqual(
			requests[0]["codex_bin"] as? String,
			"/Applications/ChatGPT.app/Contents/Resources/codex"
		)
		XCTAssertEqual(requests[0]["login_method"] as? String, "device_code")
		XCTAssertEqual(
			Set(requests[1].keys),
			["schema", "operation", "session_id"]
		)
		XCTAssertEqual(
			Set(requests[2].keys),
			["schema", "operation", "session_id"]
		)
		XCTAssertEqual(
			recorder.requests.map(\.authority),
			Array(repeating: authority, count: 3)
		)
	}

	func testAccountReauthenticationDecodesClosedFailure() async throws {
		let authority = authority
		let sessionID = "55555555-5555-4555-8555-555555555555"
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "poll_account_reauthentication",
				authority: authority,
				data: """
					{"session_id":"\(sessionID)","state":"failed",
					 "failure":"account_mismatch"}
					"""
			)
		}

		let status = try await client.pollAccountReauthentication(
			authority: authority,
			sessionID: sessionID
		)

		XCTAssertEqual(status.state, .failed)
		XCTAssertEqual(status.failure, .accountMismatch)
	}

	func testAccountEnrollmentDecodesDuplicateProviderFailure() async throws {
		let authority = authority
		let sessionID = "55555555-5555-4555-8555-555555555555"
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "poll_account_reauthentication",
				authority: authority,
				data: """
					{"session_id":"\(sessionID)","state":"failed",
					 "failure":"provider_already_enrolled"}
					"""
			)
		}

		let status = try await client.pollAccountReauthentication(
			authority: authority,
			sessionID: sessionID
		)

		XCTAssertEqual(status.state, .failed)
		XCTAssertEqual(status.failure, .providerAlreadyEnrolled)
		XCTAssertEqual(
			status.failure?.presentation,
			"This Codex login is already added. Choose a different account on the login page, then try again."
		)
	}

	func testAccountReauthenticationRejectsMalformedOrUnexpectedPromptState() async {
		let authority = authority
		let sessionID = "55555555-5555-4555-8555-555555555555"
		let malformed = [
			"""
			{"session_id":"\(sessionID)","state":"waiting_for_browser",
			 "prompt":{"verification_url":"http://auth.openai.com/codex/device",
			 "user_code":"AB12-CDE34"}}
			""",
			"""
			{"session_id":"\(sessionID)","state":"waiting_for_browser",
			 "prompt":{"verification_url":"https://auth.openai.com/codex/device",
			 "user_code":"AB12-cde34"}}
			""",
			"""
			{"session_id":"\(sessionID)","state":"completed",
			 "prompt":{"verification_url":"https://auth.openai.com/codex/device",
			 "user_code":"AB12-CDE34"}}
			""",
			"""
			{"session_id":"\(sessionID)","state":"installing",
			 "prompt":{"verification_url":"https://auth.openai.com/codex/device",
			 "user_code":"AB12-CDE34"}}
			""",
			"""
			{"session_id":"\(sessionID)","state":"failed","failure":"future_failure"}
			""",
			"""
			{"session_id":"\(sessionID)","state":"opening_browser","unexpected":true}
			""",
		]

		for data in malformed {
			let client = DecodexNativeClient { _, _ in
				nativeSuccess(
					operation: "poll_account_reauthentication",
					authority: authority,
					data: data
				)
			}
			do {
				_ = try await client.pollAccountReauthentication(
					authority: authority,
					sessionID: sessionID
				)
				XCTFail("Malformed login status must fail")
			} catch let error as AccountControlError {
				XCTAssertEqual(error, .invalidResponse)
			} catch {
				XCTFail("Unexpected error: \(error)")
			}
		}
	}
}

private func controlAccountJSON(
	accountID: String,
	alias: String,
	revision: UInt64,
	enabled: Bool = true
) -> String {
	"""
	{"account_id":"\(accountID)","alias":"\(alias)","enabled":\(enabled),
	 "account_revision":\(revision),"observed_state":"available","lifecycle_readiness":"ready",
	 "credential_binding":{"schema_version":1,"version":3,
	   "fingerprint_sha256":"\(String(repeating: "a", count: 64))",
	   "provider":"chatgpt","provider_account_id":"provider-a"},
	 "five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
	 "seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}
	"""
}

private func controlUnsettledAccountJSON(
	accountID: String,
	operationID: String
) -> String {
	"""
	{"account_id":"\(accountID)","alias":"Jamie","enabled":true,
	 "account_revision":8,"observed_state":"unknown","lifecycle_readiness":"operation_unsettled",
	 "credential_binding":{"schema_version":1,"version":2,
	   "fingerprint_sha256":"\(String(repeating: "b", count: 64))",
	   "provider":"chatgpt","provider_account_id":"provider-b"},
	 "unsettled_operation":{"operation_id":"\(operationID)","kind":"refresh",
	   "phase":"recovery_required","recovery_code":"provider_identity_changed"},
	 "five_hour_quota":{"duration_minutes":300,"observed_at_unix_micros":null,"result":{"state":"unknown"}},
	 "seven_day_quota":{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}}
	"""
}

private func controlRoutingAppliedJSON(
	revision: UInt64,
	mode: String,
	order: [String]
) -> String {
	let orderJSON = order.map { "\"\($0)\"" }.joined(separator: ",")
	return """
	{"outcome":"applied","data":{"entity_revision":\(revision),
	  "result":{"name":"account_routing_changed","data":{"routing":{
	    "revision":\(revision),"mode":\(mode),"order":[\(orderJSON)]
	  }}}}}
	"""
}
