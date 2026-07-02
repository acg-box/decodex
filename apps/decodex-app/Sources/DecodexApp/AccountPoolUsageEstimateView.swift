import Foundation
import SwiftUI

struct AccountPoolUsageEstimateView: View {
	let estimate: AccountUsageEstimate
	let accounts: [CodexAccount]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 3) {
			HStack(spacing: 5) {
				ForEach(Array(metrics.enumerated()), id: \.offset) { index, metric in
					AccountPoolUsageMetricView(
						title: metric.title,
						value: metric.value,
						tint: metric.tint
					)

					if index < metrics.count - 1 {
						Spacer(minLength: 3)
					}
				}
			}
			.frame(height: 16)

			if estimate.accountEstimateCount < estimate.accountCount {
				Text("\(estimate.accountEstimateCount)/\(estimate.accountCount) accounts measured")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
					.lineLimit(1)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityLabel)
	}

	private var metrics: [(title: String, value: String, tint: Color)] {
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

	private var accessibilityLabel: String {
		"Pool usage over \(estimate.windowDays) days: \(formatUsagePercent(estimate.totalUsedOfCapacityPercent)) used, daily change \(dayDeltaText), average \(formatUsagePercent(estimate.averageDailyPoolPercent)) per day"
	}

	private var dayDeltaText: String {
		guard let delta = dayDeltaPercentagePoints else {
			return "-"
		}

		return formatPercentagePointDelta(delta)
	}

	private var poolUsageTint: Color {
		let used = estimate.totalUsedOfCapacityPercent
		if used >= 90 {
			return PanelPalette.destructive(colorScheme)
		}
		if used >= 75 {
			return PanelPalette.warning(colorScheme)
		}

		return PanelPalette.routeAccent(colorScheme)
	}

	private var dayDeltaTint: Color {
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

	private var dayDeltaPercentagePoints: Double? {
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

	private func usageRecord(
		for account: CodexAccount,
		on date: String
	) -> AccountUsageRecord? {
		account.recentUsageRecords
			.filter { record in record.date == date }
			.max { left, right in
				left.checkedAtUnixEpoch < right.checkedAtUnixEpoch
			}
	}

	private func previousUsageDate(before value: String) -> String? {
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

private struct AccountPoolUsageMetricView: View {
	let title: String
	let value: String
	let tint: Color
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 3) {
			Text(title)
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
				.lineLimit(1)

			Text(value)
				.font(PanelFont.usageValue)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.94 : 0.78))
				.monospacedDigit()
				.lineLimit(1)
				.minimumScaleFactor(0.72)
		}
		.lineLimit(1)
	}
}
