import Foundation
import SwiftUI

struct AccountProfileAggregate: Equatable {
	let accountCount: Int
	let lifetimeTokens: Int?
	let peakDailyTokens: Int?
	let longestTaskSeconds: Int?
	let currentStreakDays: Int?
	let longestStreakDays: Int?
	let dailyUsage: [AccountProfileDailyUsage]

	static func make(accounts: [CodexAccount]) -> AccountProfileAggregate? {
		var lifetimeTokens: Int?
		var peakFallbackTokens: Int?
		var longestTaskSeconds: Int?
		var currentStreakDays: Int?
		var longestStreakDays: Int?
		var usageByDate: [String: Int] = [:]

		for account in accounts {
			if let value = account.profileLifetimeTokens {
				lifetimeTokens = (lifetimeTokens ?? 0) + value
			}
			if let value = account.profilePeakDailyTokens {
				peakFallbackTokens = (peakFallbackTokens ?? 0) + value
			}
			if let value = account.profileLongestTaskSeconds {
				longestTaskSeconds = max(longestTaskSeconds ?? 0, value)
			}
			if let value = account.profileCurrentStreakDays {
				currentStreakDays = max(currentStreakDays ?? 0, value)
			}
			if let value = account.profileLongestStreakDays {
				longestStreakDays = max(longestStreakDays ?? 0, value)
			}
			for record in account.recentProfileDailyUsage {
				usageByDate[record.date, default: 0] += record.tokens
			}
		}

		let dailyUsage = usageByDate
			.map { AccountProfileDailyUsage(date: $0.key, tokens: $0.value) }
			.sorted { $0.date < $1.date }
		let peakDailyTokens = dailyUsage.map(\.tokens).max() ?? peakFallbackTokens
		let aggregate = AccountProfileAggregate(
			accountCount: accounts.count,
			lifetimeTokens: lifetimeTokens,
			peakDailyTokens: peakDailyTokens,
			longestTaskSeconds: longestTaskSeconds,
			currentStreakDays: currentStreakDays,
			longestStreakDays: longestStreakDays,
			dailyUsage: dailyUsage
		)

		return aggregate.hasProfileSummary ? aggregate : nil
	}

	var hasProfileSummary: Bool {
		lifetimeTokens != nil
			|| peakDailyTokens != nil
			|| longestTaskSeconds != nil
			|| currentStreakDays != nil
			|| longestStreakDays != nil
			|| dailyUsage.isEmpty == false
	}
}

struct AccountTelemetryMatrixView: View {
	let aggregate: AccountProfileAggregate?
	let usageEstimate: AccountUsageEstimate?
	let accounts: [CodexAccount]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: AccountPanelLayout.telemetryRowSpacing) {
			if let aggregate {
				AccountProfileOverviewView(aggregate: aggregate)
			}

			if let usageEstimate {
				AccountPoolUsageEstimateView(estimate: usageEstimate, accounts: accounts)
			}
		}
		.padding(.horizontal, AccountPanelLayout.telemetryHorizontalPadding)
		.padding(.top, AccountPanelLayout.telemetryTopPadding)
		.padding(.bottom, AccountPanelLayout.telemetryBottomPadding)
		.frame(maxWidth: .infinity, alignment: .leading)
		.background {
			RoundedRectangle(cornerRadius: 9, style: .continuous)
				.fill(surfaceFill)
		}
		.clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
		.id(colorScheme == .dark ? "telemetry-matrix-dark" : "telemetry-matrix-light")
	}

	private var surfaceFill: Color {
		colorScheme == .dark
			? Color(red: 0.08, green: 0.095, blue: 0.13).opacity(0.34)
			: Color(red: 0.9, green: 0.94, blue: 0.98).opacity(0.48)
	}
}

private struct AccountProfileOverviewView: View {
	let aggregate: AccountProfileAggregate
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 5) {
			HStack(alignment: .firstTextBaseline, spacing: 5) {
				PanelMetricIconView(
					symbol: "sum",
					tint: PanelPalette.usageCyan(colorScheme).opacity(0.9)
				)

				Text("All accounts")
					.font(PanelFont.metricLabel)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)

				Spacer(minLength: 6)

				Text("\(aggregate.accountCount) accounts")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
					.lineLimit(1)
			}

			HStack(spacing: 5) {
				ForEach(Array(metrics.enumerated()), id: \.offset) { index, metric in
					HStack(alignment: .firstTextBaseline, spacing: 3) {
						Text(metric.label)
							.font(PanelFont.usageLabel)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
							.lineLimit(1)

						Text(metric.value)
							.font(PanelFont.usageValue)
							.foregroundStyle(index == 0 ? primaryMetricColor : PanelPalette.secondaryText(colorScheme))
							.monospacedDigit()
							.lineLimit(1)
							.minimumScaleFactor(0.72)
					}

					if index < metrics.count - 1 {
						Spacer(minLength: 3)
					}
				}
			}
			.frame(height: 16)

			if aggregate.dailyUsage.isEmpty == false {
				AccountProfileDailyUsageStripView(records: aggregate.dailyUsage)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityLabel)
	}

	private var metrics: [(label: String, value: String)] {
		[
			aggregate.lifetimeTokens.map { ("tok", formatCompactCount($0)) },
			aggregate.peakDailyTokens.map { ("peak", formatCompactCount($0)) },
			streakText.map { ("streak", $0) },
			aggregate.longestTaskSeconds
				.flatMap(formatActivityDuration)
				.map { ("task", $0) },
		]
		.compactMap { $0 }
	}

	private var streakText: String? {
		if let current = aggregate.currentStreakDays,
			let longest = aggregate.longestStreakDays
		{
			return "\(current)/\(longest)d"
		}
		if let current = aggregate.currentStreakDays {
			return "\(current)d"
		}
		if let longest = aggregate.longestStreakDays {
			return "\(longest)d"
		}

		return nil
	}

	private var primaryMetricColor: Color {
		PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.92 : 0.86)
	}

	private var accessibilityLabel: String {
		"All account profile totals, " + metrics.map { "\($0.label) \($0.value)" }.joined(separator: ", ")
	}
}

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

struct AccountPoolUsageMetricView: View {
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

