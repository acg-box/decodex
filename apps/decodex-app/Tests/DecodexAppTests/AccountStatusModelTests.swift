@testable import DecodexApp
import XCTest

final class AccountStatusModelTests: XCTestCase {
	func testRefresh401RequiresReloginAndHidesCapacity() {
		let account = makeAccount(
			status: "unusable",
			recoveryAction: "login",
			refreshStatus: "failed",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertTrue(account.needsLogin)
		XCTAssertFalse(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "login")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testExpiredAccountCanRefreshButDoesNotShowCapacity() {
		let account = makeAccount(
			status: "expired",
			recoveryAction: "refresh",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertFalse(account.needsLogin)
		XCTAssertTrue(account.canRouteRuns)
		XCTAssertEqual(account.statusLabel, "refresh")
		XCTAssertNil(account.currentCapacityLabel)
	}

	func testAvailableMeasuredAccountShowsCapacity() {
		let account = makeAccount(
			status: "available",
			planType: "pro",
			checkedAtUnixEpoch: 1_800_000_000,
			primaryRemainingPercent: 89
		)

		XCTAssertEqual(account.currentCapacityLabel, "20x")
	}

	func testProfilePeakFallsBackToDailyUsageBuckets() {
		let account = makeAccount(
			status: "available",
			profileDailyUsage: [
				AccountProfileDailyUsage(date: "2026-05-30", tokens: 123_456),
				AccountProfileDailyUsage(date: "2026-05-31", tokens: 789_000),
			]
		)

		XCTAssertEqual(account.profilePeakDailyTokensForDisplay, 789_000)
	}

	func testProfilePeakUsesExplicitStatsValueFirst() {
		let account = makeAccount(
			status: "available",
			profilePeakDailyTokens: 1_500_000,
			profileDailyUsage: [
				AccountProfileDailyUsage(date: "2026-05-30", tokens: 2_000_000),
			]
		)

		XCTAssertEqual(account.profilePeakDailyTokensForDisplay, 1_500_000)
	}

	func testUsageResetDisplayUsesInjectedClock() {
		let base = Date(timeIntervalSince1970: 1_800_000_000)
		let thirteenMinutesLater = Int(base.timeIntervalSince1970) + 780

		let pending = UsageResetDisplay.make(
			resetAtUnixEpoch: thirteenMinutesLater,
			now: base
		)
		let due = UsageResetDisplay.make(
			resetAtUnixEpoch: thirteenMinutesLater,
			now: base.addingTimeInterval(781)
		)

		XCTAssertEqual(pending.short, "13m")
		XCTAssertEqual(due.short, "0m")
		XCTAssertTrue(due.accessibility.contains("reset due now"))
	}
}
