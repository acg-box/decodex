@testable import DecodexApp
import Foundation
import XCTest

final class QuotaApplicabilityTests: XCTestCase {
	func testNotApplicableHasNoPercentageOrResetAndStaysVisible() {
		let window = ResetCardQuotaWindow(
			durationMinutes: 300, observedAtUnixMicros: 1_000_000, state: .notApplicable
		)
		let presentation = ResetCardQuotaPresentation(window: window)
		XCTAssertTrue(presentation.isVisible)
		XCTAssertEqual(presentation.valueText, "Not applicable")
		XCTAssertEqual(presentation.detailText, "No 5-hour limit reported")
		XCTAssertNil(presentation.usedPercent)
		XCTAssertNil(presentation.remainingPercent)
		XCTAssertNil(presentation.resetDate)
		XCTAssertNil(window.usedPercent)
		XCTAssertNil(window.resetDate)
		XCTAssertEqual(window.stateLabel, "Not applicable")
		XCTAssertEqual(window.accessibilityValue, "Not applicable, no 5-hour limit reported")
	}

	func testNotApplicableWinsOverOlderNumericInventoryInEitherSource() {
		let authority = ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
		let absent = ResetCardQuotaWindow(durationMinutes: 300, observedAtUnixMicros: 2_000_000, state: .notApplicable)
		let numeric = ResetCardQuotaWindow(durationMinutes: 300, observedAtUnixMicros: 1_000_000, state: .current(usedPercent: 100, resetsAtUnixMicros: 3_000_000))
		for (accountQuota, inventoryQuota) in [(absent, numeric), (numeric, absent)] {
			let account = ResetCardAccountRecord(
				authority: authority, accountID: "11111111-1111-4111-8111-111111111111",
				alias: "Test", accountRevision: 8, enabled: true, observedState: .available,
				lifecycleReadiness: .ready, fiveHourQuota: accountQuota,
				sevenDayQuota: .unknown(durationMinutes: 10_080)
			)
			let inventory = ResetCardInventory(
				authority: authority, accountID: account.accountID, accountRevision: 7, cards: [],
				fiveHourQuota: inventoryQuota, sevenDayQuota: .unknown(durationMinutes: 10_080), observationError: nil
			)
			let state = ResetCardAccountState(account: account, inventory: inventory, error: nil, isRefreshing: true)
			XCTAssertEqual(state.fiveHourQuota, absent)
		}
	}

	func testNativeNotApplicableRequiresObservedFiveHourWindow() async throws {
		let absent = #"{"duration_minutes":300,"observed_at_unix_micros":1000000,"result":{"state":"not_applicable"}}"#
		let unknownWeek = #"{"duration_minutes":10080,"observed_at_unix_micros":null,"result":{"state":"unknown"}}"#
		let inventory = try await readInventory(fiveHour: absent, sevenDay: unknownWeek)
		XCTAssertEqual(inventory.fiveHourQuota.state, .notApplicable)
		XCTAssertNil(inventory.fiveHourQuota.usedPercent)
		XCTAssertNil(inventory.fiveHourQuota.resetDate)
		let malformed = [
			(absent.replacingOccurrences(of: "1000000", with: "null"), unknownWeek),
			(absent.replacingOccurrences(of: "1000000", with: "0"), unknownWeek),
			(absent.replacingOccurrences(of: #""state":"not_applicable""#, with: #""state":"not_applicable","data":{"used_percent":0}"#), unknownWeek),
			(absent, absent.replacingOccurrences(of: "300", with: "10080")),
		]
		for (fiveHour, sevenDay) in malformed {
			do {
				_ = try await readInventory(fiveHour: fiveHour, sevenDay: sevenDay)
				XCTFail("Invalid absence projection must be rejected")
			} catch {
				XCTAssertEqual(error as? ResetCardClientError, .invalidResponse)
			}
		}
	}

	private func readInventory(fiveHour: String, sevenDay: String) async throws -> ResetCardInventory {
		let authority = ResetCardAuthority(profileName: "local", serverID: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
		let accountID = "11111111-1111-4111-8111-111111111111"
		let client = DecodexNativeClient { _, _ in
			nativeSuccess(operation: "get_reset_cards", authority: authority, data: """
			{"outcome":"available","data":{
			"account_id":"\(accountID)","account_revision":7,
			"reported_available_count":0,"details_complete":true,"cards":[],
			"five_hour_quota":\(fiveHour),"seven_day_quota":\(sevenDay)
			}}
			""")
		}
		return try await client.inventory(for: nativeAccount(authority: authority, accountID: accountID, revision: 7))
	}
}
