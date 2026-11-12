@testable import DecodexApp
import Foundation
import XCTest

final class ConversationEstimatesTests: XCTestCase {
	func testCompactDollarsKeepSmallNonzeroAmountsAndCarryAtACent() {
		let cases: [(UInt64, String)] = [
			(0, "$0.00"), (1, "< $0.01"), (9_999, "< $0.01"),
			(10_000, "$0.01"), (351_369, "$0.35"), (999_999, "$1.00"),
			(1_234_567, "$1.23"), (UInt64.max, "$18446744073709.55"),
		]
		for (value, expected) in cases {
			XCTAssertEqual(ConversationEstimateRow.currency(value), expected)
		}
	}

	func testApiEquivalentDecodesItsPartialCoverage() throws {
		let row = try JSONDecoder().decode(ConversationEstimateRow.self, from: Data(#"{"work_id":"agent","title":"Build","account_id":"account","usd_micros":329020,"native_usd_micros":null,"credits_micros":null,"observed_at_micros":1,"api_equivalent":true,"partial":true}"#.utf8))
		XCTAssertEqual(row.amount, "≈ $0.33+")
		XCTAssertEqual(row.api_equivalent, true)
		XCTAssertEqual(row.partial, true)
	}

	func testUnknownCostAndReportedZeroRemainDifferent() throws {
		func row(_ money: String) throws -> ConversationEstimateRow {
			try JSONDecoder().decode(ConversationEstimateRow.self, from: Data("""
			{"work_id":"agent","title":"Build","account_id":"account","usd_micros":\(money),"credits_micros":null,"observed_at_micros":1}
			""".utf8))
		}
		XCTAssertNil(try row("null").amount)
		XCTAssertEqual(try row("0").amount, "≈ $0.00")
		XCTAssertEqual(try row("1234567").amount, "≈ $1.23")
		let credits = ConversationEstimateRow(work_id: "agent", title: "Build", account_id: "account", usd_micros: nil, native_usd_micros: nil, credits_micros: 46_000_000, observed_at_micros: 1, api_equivalent: nil, partial: nil)
		XCTAssertNil(credits.amount)
		let native = ConversationEstimateRow(work_id: "agent", title: "Build", account_id: "account", usd_micros: 9_000_000, native_usd_micros: 0, credits_micros: 46_000_000, observed_at_micros: 1, api_equivalent: nil, partial: nil)
		XCTAssertEqual(native.amount, "$0.00")
	}

	func testCostSectionRequiresReportedMoneyAndRetainsReportedZero() throws {
		let unknown = ConversationEstimateRow(work_id: "unknown", title: "Unknown", account_id: nil, usd_micros: nil, native_usd_micros: nil, credits_micros: 46_000_000, observed_at_micros: 1, api_equivalent: nil, partial: nil)
		let zero = ConversationEstimateRow(work_id: "zero", title: "Zero", account_id: nil, usd_micros: nil, native_usd_micros: 0, credits_micros: nil, observed_at_micros: 1, api_equivalent: nil, partial: nil)
		XCTAssertTrue(ConversationEstimatesRead(available: true, rows: [unknown], more_available: false).pricedRows.isEmpty)
		XCTAssertEqual(ConversationEstimatesRead(available: true, rows: [unknown, zero], more_available: false).pricedRows, [zero])
		XCTAssertTrue(ConversationEstimatesRead(available: false, rows: [zero], more_available: false).pricedRows.isEmpty)
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
		XCTAssertEqual(response.data.rows.first?.amount, "$1.23")
		XCTAssertEqual(try XCTUnwrap(recorder.requests.first).authority, authority)
		let request = try nativeJSONObject(XCTUnwrap(recorder.requests.first).data)
		XCTAssertEqual(Set(request.keys), ["schema", "operation"])
	}
}
