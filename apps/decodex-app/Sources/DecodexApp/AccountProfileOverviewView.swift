import SwiftUI

struct AccountProfileOverviewView: View {
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
