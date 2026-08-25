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

		XCTAssertEqual(aggregate?.lifetimeTokens, 3_000)
		XCTAssertEqual(aggregate?.peakDailyTokens, 900)
		XCTAssertEqual(aggregate?.longestTaskSeconds, 120)
		XCTAssertEqual(aggregate?.currentStreakDays, 4)
		XCTAssertEqual(aggregate?.longestStreakDays, 6)
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
	}

	func testAggregateRetainsAvailableMetricsWhenSomeProfilesArePartial() {
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

		XCTAssertEqual(aggregate?.lifetimeTokens, 1_000)
		XCTAssertEqual(aggregate?.peakDailyTokens, 900)
		XCTAssertEqual(aggregate?.longestTaskSeconds, 90)
		XCTAssertEqual(aggregate?.currentStreakDays, 2)
		XCTAssertEqual(aggregate?.longestStreakDays, 6)
		XCTAssertEqual(aggregate?.dailyUsage, [])
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

	func testCompactUsageDateUsesStablePosixPresentationWithoutAcceptingDrift() {
		XCTAssertEqual(compactUsageDate("2026-07-28"), "Jul 28")
		XCTAssertEqual(compactUsageDate("2026-13-28"), "2026-13-28")
		XCTAssertEqual(compactUsageDate("2026-07-00"), "2026-07-00")
		XCTAssertEqual(compactUsageDate("not-a-date"), "not-a-date")
	}
}
