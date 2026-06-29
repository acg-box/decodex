import SwiftUI

struct AccountProfileSummaryView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(spacing: 4) {
			if metrics.isEmpty == false {
				HStack(alignment: .firstTextBaseline, spacing: 5) {
					PanelMetricIconView(
						symbol: "chart.bar.xaxis",
						tint: PanelPalette.secondaryText(colorScheme).opacity(0.82)
					)

					ForEach(Array(metrics.enumerated()), id: \.offset) { index, metric in
						HStack(alignment: .firstTextBaseline, spacing: 3) {
							Text(metric.label)
								.font(PanelFont.usageLabel)
								.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
								.lineLimit(1)

							Text(metric.value)
								.font(PanelFont.usageValue)
								.foregroundStyle(valueColor(index: index))
								.monospacedDigit()
								.lineLimit(1)
						}

						if index < metrics.count - 1 {
							Spacer(minLength: 3)
						}
					}
				}
				.frame(height: 16)
			}

			if account.recentProfileDailyUsage.isEmpty == false {
				AccountProfileDailyUsageStripView(records: account.recentProfileDailyUsage)
			}
		}
		.accessibilityLabel(accessibilityLabel)
	}

	private var metrics: [(label: String, value: String)] {
		[
			account.profileLifetimeTokens.map { ("tok", formatCompactCount($0)) },
			account.profilePeakDailyTokensForDisplay.map { ("peak", formatCompactCount($0)) },
			streakText.map { ("streak", $0) },
			account.profileLongestTaskSeconds
				.flatMap(formatActivityDuration)
				.map { ("task", $0) },
		]
		.compactMap { $0 }
	}

	private var streakText: String? {
		if let current = account.profileCurrentStreakDays,
			let longest = account.profileLongestStreakDays
		{
			return "\(current)/\(longest)d"
		}
		if let current = account.profileCurrentStreakDays {
			return "\(current)d"
		}
		if let longest = account.profileLongestStreakDays {
			return "\(longest)d"
		}

		return nil
	}

	private var accessibilityLabel: String {
		metrics.map { "\($0.label) \($0.value)" }.joined(separator: ", ")
	}

	private func valueColor(index: Int) -> Color {
		index == 0
			? PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.92 : 0.86)
			: PanelPalette.secondaryText(colorScheme)
	}
}

struct AccountProfileDailyUsageStripView: View {
	let records: [AccountProfileDailyUsage]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 2) {
			ForEach(Array(displayRecords.enumerated()), id: \.offset) { _, record in
				RoundedRectangle(cornerRadius: 2, style: .continuous)
					.fill(tileColor(tokens: record.tokens))
					.frame(width: 6, height: 9)
					.help("\(compactUsageDate(record.date)): \(formatCompactCount(record.tokens)) tokens")
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.frame(height: 11)
		.accessibilityHidden(true)
	}

	private var displayRecords: [AccountProfileDailyUsage] {
		Array(records.sorted { $0.date < $1.date }.suffix(36))
	}

	private var peakTokens: Int {
		max(1, displayRecords.map(\.tokens).max() ?? 1)
	}

	private func tileColor(tokens: Int) -> Color {
		let intensity = max(0.16, min(1, Double(tokens) / Double(peakTokens)))
		return PanelPalette.usageCyan(colorScheme).opacity(0.24 + 0.62 * intensity)
	}
}
