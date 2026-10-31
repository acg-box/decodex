@testable import DecodexApp
import Foundation
import XCTest

final class ConversationEstimatesTests: XCTestCase {
	func testUnknownCostAndReportedZeroRemainDifferent() throws {
		func row(_ money: String) throws -> ConversationEstimateRow {
			try JSONDecoder().decode(ConversationEstimateRow.self, from: Data("""
			{"work_id":"agent","title":"Build","account_id":"account","usd_micros":\(money),"credits_micros":null,"observed_at_micros":1}
			""".utf8))
		}
		XCTAssertNil(try row("null").amount)
		XCTAssertEqual(try row("0").amount, "≈ $0")
		XCTAssertEqual(try row("1234567").amount, "≈ $1.234567")
		let credits = ConversationEstimateRow(work_id: "agent", title: "Build", account_id: "account", usd_micros: nil, native_usd_micros: nil, credits_micros: 46_000_000, observed_at_micros: 1)
		XCTAssertNil(credits.amount)
		let native = ConversationEstimateRow(work_id: "agent", title: "Build", account_id: "account", usd_micros: 9_000_000, native_usd_micros: 0, credits_micros: 46_000_000, observed_at_micros: 1)
		XCTAssertEqual(native.amount, "$0")
	}

	func testEstimateReadUsesTheExistingPinnedNativeAuthority() async throws {
		let authority = ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
		let recorder = NativeRequestRecorder()
		let client = DecodexNativeClient { request, pin in
			recorder.append(request, authority: pin)
			return nativeSuccess(operation: "get_conversation_estimates", authority: authority,
				data: #"{"available":true,"rows":[{"work_id":"agent","title":"Build","account_id":"account","usd_micros":null,"native_usd_micros":1234567,"credits_micros":46000000,"observed_at_micros":1}],"more_available":true}"#)
		}
		let response = try await client.perform(DecodexNativeRequest(operation: "get_conversation_estimates"), authority: authority, as: ConversationEstimatesRead.self)
		XCTAssertTrue(response.data.more_available)
		XCTAssertEqual(response.data.rows.first?.amount, "$1.234567")
		XCTAssertEqual(try XCTUnwrap(recorder.requests.first).authority, authority)
		let request = try nativeJSONObject(XCTUnwrap(recorder.requests.first).data)
		XCTAssertEqual(Set(request.keys), ["schema", "operation"])
	}
}
