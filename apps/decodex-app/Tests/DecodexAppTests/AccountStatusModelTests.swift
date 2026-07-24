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

	func testResetCreditsUseReportedCountAndSortByExpiry() {
		let account = makeAccount(
			status: "available",
			resetCreditsAvailableCount: 4,
			resetCredits: [
				AccountResetCredit(
					grantedAtUnixEpoch: 1_782_435_600,
					expiresAtUnixEpoch: 1_785_027_600,
					status: "available"
				),
				AccountResetCredit(
					grantedAtUnixEpoch: 1_782_366_690,
					expiresAtUnixEpoch: 1_784_958_690,
					status: "available"
				),
				AccountResetCredit(
					grantedAtUnixEpoch: 1_782_366_690,
					expiresAtUnixEpoch: 1_784_958_690,
					status: "redeemed"
				),
			]
		)

		XCTAssertTrue(account.hasResetCreditsSummary)
		XCTAssertTrue(account.hasUsageSummary)
		XCTAssertEqual(account.visibleResetCreditCount, 4)
		XCTAssertEqual(
			account.availableResetCredits.map(\.expiresAtUnixEpoch),
			[1_784_958_690, 1_785_027_600]
		)
	}

	func testResetCreditsPreserveDuplicateExpiryCards() {
		let account = makeAccount(
			status: "available",
			resetCreditsAvailableCount: 2,
			resetCredits: [
				AccountResetCredit(
					grantedAtUnixEpoch: 1_782_366_690,
					expiresAtUnixEpoch: 1_784_958_690,
					status: "available"
				),
				AccountResetCredit(
					grantedAtUnixEpoch: 1_782_366_690,
					expiresAtUnixEpoch: 1_784_958_690,
					status: "available"
				),
			]
		)

		XCTAssertEqual(account.visibleResetCreditCount, 2)
		XCTAssertEqual(account.availableResetCredits.count, 2)
		XCTAssertEqual(
			account.availableResetCredits.map(\.expiresAtUnixEpoch),
			[1_784_958_690, 1_784_958_690]
		)
	}

	func testResetDatesUseProvidedTimeZone() throws {
		let resetAtUnixEpoch = 1_784_958_690
		let now = Date(timeIntervalSince1970: 1_784_950_000)
		let newYork = try XCTUnwrap(TimeZone(identifier: "America/New_York"))
		let shanghai = try XCTUnwrap(TimeZone(identifier: "Asia/Shanghai"))

		XCTAssertEqual(
			formatResetCreditDate(resetAtUnixEpoch, timeZone: newYork),
			"Jul 25 01:51"
		)
		XCTAssertEqual(
			UsageResetDisplay.make(
				resetAtUnixEpoch: resetAtUnixEpoch,
				now: now,
				timeZone: newYork
			).date,
			"Jul 25 01:51"
		)
		XCTAssertEqual(
			formatResetCreditDate(resetAtUnixEpoch, timeZone: shanghai),
			"Jul 25 13:51"
		)
		XCTAssertEqual(
			UsageResetDisplay.make(
				resetAtUnixEpoch: resetAtUnixEpoch,
				now: now,
				timeZone: shanghai
			).date,
			"Jul 25 13:51"
		)
		XCTAssertEqual(formatResetCreditDate(nil, timeZone: newYork), "-")
	}

	func testUsageResetDateUsesProvidedTimeZoneAtYearBoundary() throws {
		let now = Date(timeIntervalSince1970: 1_798_731_000)
		let resetAtUnixEpoch = 1_798_734_600
		let newYork = try XCTUnwrap(TimeZone(identifier: "America/New_York"))
		let shanghai = try XCTUnwrap(TimeZone(identifier: "Asia/Shanghai"))

		XCTAssertEqual(
			UsageResetDisplay.make(
				resetAtUnixEpoch: resetAtUnixEpoch,
				now: now,
				timeZone: newYork
			).date,
			"Dec 31 11:30"
		)
		XCTAssertEqual(
			UsageResetDisplay.make(
				resetAtUnixEpoch: resetAtUnixEpoch,
				now: now,
				timeZone: shanghai
			).date,
			"Jan 1 2027 00:30"
		)
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
