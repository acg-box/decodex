import Foundation
import SwiftUI

extension AccountPoolUsageEstimateView {
	var metrics: [(title: String, value: String, tint: Color)] {
		[
			(
				"Pool used",
				formatUsagePercent(estimate.totalUsedOfCapacityPercent),
				poolUsageTint
			),
			("Day Δ", dayDeltaText, dayDeltaTint),
			(
				"Daily avg",
				formatDailyUsageRate(estimate.averageDailyPoolPercent),
				PanelPalette.secondaryText(colorScheme)
			),
		]
	}

	var accessibilityLabel: String {
		"Pool usage over \(estimate.windowDays) days: \(formatUsagePercent(estimate.totalUsedOfCapacityPercent)) used, daily change \(dayDeltaText), average \(formatUsagePercent(estimate.averageDailyPoolPercent)) per day"
	}

	var dayDeltaText: String {
		guard let delta = dayDeltaPercentagePoints else {
			return "-"
		}

		return formatPercentagePointDelta(delta)
	}

	var poolUsageTint: Color {
		let used = estimate.totalUsedOfCapacityPercent
		if used >= 90 {
			return PanelPalette.destructive(colorScheme)
		}
		if used >= 75 {
			return PanelPalette.warning(colorScheme)
		}

		return PanelPalette.routeAccent(colorScheme)
	}

	var dayDeltaTint: Color {
		guard let delta = dayDeltaPercentagePoints else {
			return PanelPalette.secondaryText(colorScheme)
		}
		if delta > 0.05 {
			if estimate.totalUsedOfCapacityPercent >= 90 {
				return PanelPalette.destructive(colorScheme)
			}
			if estimate.totalUsedOfCapacityPercent >= 75 {
				return PanelPalette.warning(colorScheme)
			}

			return PanelPalette.capacityAccent(colorScheme)
		}
		if delta < -0.05 {
			return PanelPalette.secondaryText(colorScheme)
		}

		return PanelPalette.secondaryText(colorScheme)
	}

	var dayDeltaPercentagePoints: Double? {
		let measuredAccounts = accounts.filter { account in
			account.sevenDayUsedPercent != nil
		}
		guard measuredAccounts.isEmpty == false, estimate.totalCapacityPercent > 0 else {
			return nil
		}

		let latestDate = measuredAccounts
			.flatMap(\.recentUsageRecords)
			.map(\.date)
			.max()
		guard let latestDate else {
			return estimate.averageDailyPoolPercent
		}
		guard let previousDate = previousUsageDate(before: latestDate) else {
			return estimate.averageDailyPoolPercent
		}

		let previousRecords = measuredAccounts.compactMap { account in
			usageRecord(for: account, on: previousDate).map { (account, $0) }
		}
		guard previousRecords.count == measuredAccounts.count else {
			return estimate.averageDailyPoolPercent
		}
		let previousUsedPercent = previousRecords.reduce(0) { total, pair in
			let (account, record) = pair

			return total + record.usedPercent * (record.capacityMultiplier ?? account.capacityWeight)
		}
		let previousPoolPercent =
			(Double(previousUsedPercent) / Double(estimate.totalCapacityPercent)) * 100

		return estimate.totalUsedOfCapacityPercent - previousPoolPercent
	}

	func usageRecord(
		for account: CodexAccount,
		on date: String
	) -> AccountUsageRecord? {
		account.recentUsageRecords
			.filter { record in record.date == date }
			.max { left, right in
				left.checkedAtUnixEpoch < right.checkedAtUnixEpoch
			}
	}

	func previousUsageDate(before value: String) -> String? {
		let formatter = DateFormatter()
		formatter.locale = Locale(identifier: "en_US_POSIX")
		formatter.dateFormat = "yyyy-MM-dd"
		let calendar = Calendar(identifier: .gregorian)

		guard let date = formatter.date(from: value),
			let previousDate = calendar.date(byAdding: .day, value: -1, to: date)
		else {
			return nil
		}

		return formatter.string(from: previousDate)
	}
}
