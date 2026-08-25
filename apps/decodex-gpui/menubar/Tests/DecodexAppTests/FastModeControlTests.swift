@testable import DecodexApp
import Foundation
import XCTest

final class FastModeControlTests: XCTestCase {
	private let authority = ResetCardAuthority(
		profileName: "local",
		serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
	)

	func testNativeFastModeStatusUsesExactRequestAndReadsSuccess() async throws {
		let authority = authority
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "fast_mode_status",
				authority: authority,
				data: #"{"enabled":true}"#
			)
		}

		let status = try await client.status()
		XCTAssertTrue(status)
		let request = try nativeJSONObject(XCTUnwrap(recorder.requests.first).data)
		XCTAssertEqual(
			request,
			[
				"schema": decodexNativeClientSchema,
				"operation": "fast_mode_status",
			]
		)
	}

	func testNativeFastModeSetUsesExactBooleanAndRejectsUnknownData() async throws {
		let authority = authority
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, requestedAuthority in
			recorder.append(request, authority: requestedAuthority)
			return nativeSuccess(
				operation: "set_fast_mode",
				authority: authority,
				data: #"{"enabled":false}"#
			)
		}

		let enabled = try await client.setEnabled(false)
		XCTAssertFalse(enabled)
		let request = try nativeJSONObject(XCTUnwrap(recorder.requests.first).data)
		XCTAssertEqual(request["operation"] as? String, "set_fast_mode")
		XCTAssertEqual(request["enabled"] as? Bool, false)
		XCTAssertEqual(Set(request.keys), ["schema", "operation", "enabled"])

		let malformed = DecodexNativeClient { _, _ in
			nativeSuccess(
				operation: "fast_mode_status",
				authority: authority,
				data: #"{"enabled":true,"extra":false}"#
			)
		}
		do {
			_ = try await malformed.status()
			XCTFail("Unknown Fast-mode fields must fail")
		} catch let error as FastModeClientError {
			XCTAssertEqual(error.localizedDescription, FastModeClientError.invalidResponse.localizedDescription)
		}
	}

	func testClosedFastModeFailureMapsToRejected() async {
		let client = DecodexNativeClient { _, _ in
			nativeFailure(operation: "set_fast_mode", failure: "write_failed")
		}

		do {
			_ = try await client.setEnabled(true)
			XCTFail("Write failure must be rejected")
		} catch let error as FastModeClientError {
			XCTAssertEqual(error.localizedDescription, FastModeClientError.rejected.localizedDescription)
		} catch {
			XCTFail("Unexpected error: \(error)")
		}
	}
}
