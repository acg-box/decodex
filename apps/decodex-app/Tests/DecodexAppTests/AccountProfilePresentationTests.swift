@testable import DecodexApp
import XCTest

final class AccountProfilePresentationTests: XCTestCase {
	func testAggregateSumsTokensAndDailyUsageButTakesLongestMetrics() {
		let aggregate = AccountProfileAggregate.make([
			AccountProfileSnapshot(
				lifetimeTokens: 1_000,
				peakDailyTokens: 500,
				longestTaskSeconds: 90,
				currentStreakDays: 2,
				longestStreakDays: 6,
				dailyUsage: [
					AccountProfileDailyUsage(date: "2026-07-27", tokens: 300),
					AccountProfileDailyUsage(date: "2026-07-28", tokens: 400),
				]
			),
			AccountProfileSnapshot(
				lifetimeTokens: 2_000,
				peakDailyTokens: 900,
				longestTaskSeconds: 120,
				currentStreakDays: 4,
				longestStreakDays: 5,
				dailyUsage: [
					AccountProfileDailyUsage(date: "2026-07-28", tokens: 800),
				]
			),
		])

		XCTAssertEqual(aggregate?.accountCount, 2)
		XCTAssertEqual(aggregate?.lifetimeTokens, 3_000)
		XCTAssertEqual(aggregate?.lifetimeTokensCoverage, 2)
		XCTAssertEqual(aggregate?.peakDailyTokens, 1_200)
		XCTAssertEqual(aggregate?.peakDailyTokensCoverage, 2)
		XCTAssertEqual(aggregate?.longestTaskSeconds, 120)
		XCTAssertEqual(aggregate?.longestTaskSecondsCoverage, 2)
		XCTAssertEqual(aggregate?.currentStreakDays, 4)
		XCTAssertEqual(aggregate?.currentStreakDaysCoverage, 2)
		XCTAssertEqual(aggregate?.longestStreakDays, 6)
		XCTAssertEqual(aggregate?.longestStreakDaysCoverage, 2)
		XCTAssertEqual(aggregate?.dailyUsageCoverage, 2)
		XCTAssertEqual(
			aggregate?.dailyUsage,
			[
				AccountProfileDailyUsage(date: "2026-07-27", tokens: 300),
				AccountProfileDailyUsage(date: "2026-07-28", tokens: 1_200),
			]
		)
	}

	func testAggregateDoesNotAddPerAccountPeaksFromDifferentDates() {
		let aggregate = AccountProfileAggregate.make([
			AccountProfileSnapshot(
				lifetimeTokens: 1,
				peakDailyTokens: 500,
				longestTaskSeconds: nil,
				currentStreakDays: nil,
				longestStreakDays: nil,
				dailyUsage: [
					AccountProfileDailyUsage(date: "2026-07-27", tokens: 500),
				]
			),
			AccountProfileSnapshot(
				lifetimeTokens: 2,
				peakDailyTokens: 900,
				longestTaskSeconds: nil,
				currentStreakDays: nil,
				longestStreakDays: nil,
				dailyUsage: [
					AccountProfileDailyUsage(date: "2026-07-28", tokens: 900),
				]
			),
		])

		XCTAssertEqual(aggregate?.peakDailyTokens, 900)
		XCTAssertEqual(aggregate?.peakDailyTokensCoverage, 2)
	}

	func testAggregateTracksPartialCoverageAndHasNoPeakWithoutDailySeries() {
		let aggregate = AccountProfileAggregate.make([
			AccountProfileSnapshot(
				lifetimeTokens: 1_000,
				peakDailyTokens: 500,
				longestTaskSeconds: 90,
				currentStreakDays: 2,
				longestStreakDays: 6,
				dailyUsage: []
			),
			AccountProfileSnapshot(
				lifetimeTokens: nil,
				peakDailyTokens: 900,
				longestTaskSeconds: nil,
				currentStreakDays: nil,
				longestStreakDays: nil,
				dailyUsage: []
			),
		])

		XCTAssertEqual(aggregate?.accountCount, 2)
		XCTAssertEqual(aggregate?.lifetimeTokens, 1_000)
		XCTAssertEqual(aggregate?.lifetimeTokensCoverage, 1)
		XCTAssertNil(aggregate?.peakDailyTokens)
		XCTAssertEqual(aggregate?.peakDailyTokensCoverage, 0)
		XCTAssertEqual(aggregate?.longestTaskSecondsCoverage, 1)
		XCTAssertEqual(aggregate?.dailyUsageCoverage, 0)
	}

	func testAggregateReturnsNilWhenNoProfileHasContent() {
		XCTAssertNil(
			AccountProfileAggregate.make([
				AccountProfileSnapshot(
					lifetimeTokens: nil,
					peakDailyTokens: nil,
					longestTaskSeconds: nil,
					currentStreakDays: nil,
					longestStreakDays: nil,
					dailyUsage: []
				),
			])
		)
	}

	func testCompactCountAndDurationFormatting() {
		XCTAssertEqual(formatCompactCount(999), "999")
		XCTAssertEqual(formatCompactCount(1_250), "1.3K")
		XCTAssertEqual(formatCompactCount(2_000_000), "2M")
		XCTAssertEqual(formatActivityDuration(45), "45s")
		XCTAssertEqual(formatActivityDuration(125), "2m 5s")
		XCTAssertEqual(formatActivityDuration(7_500), "2h 5m")
	}

	func testOverviewUsesDirectCurrentProfileLanguage() {
		XCTAssertEqual(
			AccountProfileCoveragePresentation(
				currentCount: 4,
				totalCount: 6
			).label,
			"4 of 6 profiles current"
		)
		XCTAssertEqual(
			AccountProfileCoveragePresentation(
				currentCount: 6,
				totalCount: 6
			).label,
			"6 of 6 profiles current"
		)
	}

	func testDailyUsageNormalizationKeepsFixedCalendarSlotsAndExplicitGaps() {
		let records = normalizedDailyUsage(
			[
				AccountProfileDailyUsage(date: "2026-07-25", tokens: 300),
				AccountProfileDailyUsage(date: "2026-07-28", tokens: 900),
			],
			maximumCount: 5
		)

		XCTAssertEqual(
			records,
			[
				AccountProfileDailyUsage(date: "2026-07-24", tokens: 0),
				AccountProfileDailyUsage(date: "2026-07-25", tokens: 300),
				AccountProfileDailyUsage(date: "2026-07-26", tokens: 0),
				AccountProfileDailyUsage(date: "2026-07-27", tokens: 0),
				AccountProfileDailyUsage(date: "2026-07-28", tokens: 900),
			]
		)
	}
}
