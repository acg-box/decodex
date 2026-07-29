import SwiftUI

struct AccountProfileSummaryView: View {
	let profile: AccountProfileSnapshot
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 5) {
			AccountProfileMetricsView(metrics: metrics)

			if profile.dailyUsage.isEmpty == false {
				AccountDailyUsageChart(
					records: profile.dailyUsage,
					showsAxis: false
				)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityElement(children: .combine)
	}

	private var metrics: [AccountProfileMetric] {
		AccountProfileMetric.make(
			lifetimeTokens: profile.lifetimeTokens,
			peakDailyTokens: profile.peakDailyTokens,
			longestTaskSeconds: profile.longestTaskSeconds,
			currentStreakDays: profile.currentStreakDays,
			longestStreakDays: profile.longestStreakDays
		)
	}
}

struct AccountProfileDetailView: View {
	let state: ResetCardAccountState
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 9) {
			HStack(alignment: .firstTextBaseline) {
				Text("Account details")
					.font(.headline)

				Spacer()

				if let planType {
					Text(planType)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				}
			}

			if let profile = state.profile {
				AccountProfileSummaryView(profile: profile.snapshot)

				if profile.isCached {
					Label("Saved activity", systemImage: "clock.arrow.circlepath")
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.warning(colorScheme))
				}

				if let degradationText = state.profileDegradationText {
					Text(degradationText)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.warning(colorScheme))
						.fixedSize(horizontal: false, vertical: true)
				}
			} else if state.isProfileRefreshing {
				HStack(spacing: 6) {
					ProgressView()
						.controlSize(.mini)
					Text("Loading saved activity")
						.font(PanelFont.accountDetail)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				}
			} else {
				Text("No saved activity is available for this account.")
					.font(PanelFont.accountDetail)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.fixedSize(horizontal: false, vertical: true)
			}
		}
		.frame(width: 270)
		.padding(12)
		.accessibilityElement(children: .contain)
	}

	private var planType: String? {
		state.profile?.planType ?? state.profileUnavailable?.claims.planType
	}
}

struct AccountProfileOverviewView: View {
	let aggregate: AccountProfileAggregate
	let totalAccountCount: Int
	let currentProfileCount: Int
	let degradedProfileCount: Int
	@Environment(\.colorScheme) private var colorScheme

	init(
		aggregate: AccountProfileAggregate,
		totalAccountCount: Int? = nil,
		currentProfileCount: Int? = nil,
		degradedProfileCount: Int = 0
	) {
		self.aggregate = aggregate
		self.totalAccountCount = totalAccountCount ?? aggregate.accountCount
		self.currentProfileCount = currentProfileCount ?? aggregate.accountCount
		self.degradedProfileCount = degradedProfileCount
	}

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			HStack(alignment: .firstTextBaseline, spacing: 3) {
				Image(systemName: "chart.bar.xaxis")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.usageCyan(colorScheme))
					.accessibilityHidden(true)

				Text("All accounts")
					.font(PanelFont.usageValue)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))

				Spacer(minLength: 4)

				Text(profileCountLabel)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			}

			if completeAggregateMetrics.isEmpty == false {
				AccountProfileMetricsView(
					metrics: completeAggregateMetrics
				)
			}

			if aggregate.dailyUsage.isEmpty == false {
				AccountDailyUsageChart(
					records: aggregate.dailyUsage,
					showsAxis: true,
					axisLabel: aggregate.dailyUsageCoverage == totalAccountCount
						? nil
						: "\(aggregate.dailyUsageCoverage) of \(totalAccountCount) daily"
				)
			}
		}
		.padding(.horizontal, 7)
		.padding(.vertical, 6)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
		.accessibilityElement(children: .combine)
	}

	private var profileCountLabel: String {
		AccountProfileCoveragePresentation(
			currentCount: currentProfileCount,
			totalCount: totalAccountCount
		).label
	}

	private var completeAggregateMetrics: [AccountProfileMetric] {
		AccountProfileMetric.make(
			lifetimeTokens: aggregate.lifetimeTokensCoverage == totalAccountCount
				? aggregate.lifetimeTokens
				: nil,
			peakDailyTokens: aggregate.peakDailyTokensCoverage == totalAccountCount
				? aggregate.peakDailyTokens
				: nil,
			longestTaskSeconds: aggregate.longestTaskSecondsCoverage == totalAccountCount
				? aggregate.longestTaskSeconds
				: nil,
			currentStreakDays: aggregate.currentStreakDaysCoverage == totalAccountCount
				? aggregate.currentStreakDays
				: nil,
			longestStreakDays: aggregate.longestStreakDaysCoverage == totalAccountCount
				? aggregate.longestStreakDays
				: nil
		)
	}
}

private struct AccountProfileMetric: Identifiable {
	let id: String
	let label: String
	let value: String

	static func make(
		lifetimeTokens: UInt64?,
		peakDailyTokens: UInt64?,
		longestTaskSeconds: UInt64?,
		currentStreakDays: UInt32?,
		longestStreakDays: UInt32?
	) -> [Self] {
		[
			lifetimeTokens.map {
				Self(id: "tokens", label: "total", value: formatCompactCount($0))
			},
			peakDailyTokens.map {
				Self(id: "peak", label: "peak", value: formatCompactCount($0))
			},
			streak(
				current: currentStreakDays,
				longest: longestStreakDays
			).map {
				Self(id: "streak", label: "streak", value: $0)
			},
			longestTaskSeconds.map {
				Self(id: "task", label: "task", value: formatActivityDuration($0))
			},
		]
		.compactMap { $0 }
	}

	private static func streak(current: UInt32?, longest: UInt32?) -> String? {
		switch (current, longest) {
		case (.some(let current), .some(let longest)):
			return "\(current)/\(longest)d"
		case (.some(let current), nil):
			return "\(current)d"
		case (nil, .some(let longest)):
			return "\(longest)d"
		case (nil, nil):
			return nil
		}
	}
}

private struct AccountProfileMetricsView: View {
	let metrics: [AccountProfileMetric]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		if metrics.isEmpty == false {
			HStack(alignment: .firstTextBaseline, spacing: 3) {
				ForEach(metrics) { metric in
					HStack(alignment: .firstTextBaseline, spacing: 2) {
						Text(metric.label)
							.font(PanelFont.usageLabel)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))

						Text(metric.value)
							.font(PanelFont.usageValue)
							.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.9))
							.monospacedDigit()
					}

					if metric.id != metrics.last?.id {
						Spacer(minLength: 1)
					}
				}
			}
			.lineLimit(1)
		}
	}
}

struct AccountDailyUsageChart: View {
	let records: [AccountProfileDailyUsage]
	let showsAxis: Bool
	let axisLabel: String?
	@Environment(\.colorScheme) private var colorScheme

	init(
		records: [AccountProfileDailyUsage],
		showsAxis: Bool,
		axisLabel: String? = nil
	) {
		self.records = records
		self.showsAxis = showsAxis
		self.axisLabel = axisLabel
	}

	var body: some View {
		let values = displayRecords

		VStack(alignment: .leading, spacing: 2) {
			GeometryReader { proxy in
				ZStack(alignment: .bottom) {
					Rectangle()
						.fill(PanelPalette.separator(colorScheme))
						.frame(height: 0.5)

					HStack(alignment: .bottom, spacing: 1.5) {
						ForEach(values) { record in
							RoundedRectangle(cornerRadius: 1.5, style: .continuous)
								.fill(barColor(record.tokens))
								.frame(
									maxWidth: .infinity,
									minHeight: 1,
									maxHeight: barHeight(
										record.tokens,
										available: proxy.size.height
									)
								)
								.help(
									"\(compactUsageDate(record.date)): "
										+ "\(record.tokens.formatted()) tokens"
								)
						}
					}
					.frame(maxHeight: .infinity, alignment: .bottom)
				}
			}
			.frame(height: showsAxis ? 20 : 16)

			if showsAxis {
				HStack {
					Text(values.first.map { compactUsageDate($0.date) } ?? "")
						.frame(maxWidth: .infinity, alignment: .leading)
					if let axisLabel {
						Text(axisLabel)
							.frame(maxWidth: .infinity, alignment: .center)
					}
					Text(values.last.map { compactUsageDate($0.date) } ?? "")
						.frame(maxWidth: .infinity, alignment: .trailing)
				}
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
				.lineLimit(1)
			}
		}
		.accessibilityElement(children: .ignore)
		.accessibilityLabel(
				"Daily token usage from \(values.first?.date ?? "unknown") "
					+ "through \(values.last?.date ?? "unknown"), "
					+ "\(totalTokens.formatted()) tokens total, "
					+ "\(values.map(\.tokens).max().map { $0.formatted() } ?? "0") tokens peak"
			)
	}

	private var displayRecords: [AccountProfileDailyUsage] {
		normalizedDailyUsage(records, maximumCount: 36)
	}

	private var peak: UInt64 {
		max(1, displayRecords.map(\.tokens).max() ?? 1)
	}

	private var totalTokens: UInt64 {
		displayRecords.reduce(0) {
			$0.addingWithoutOverflow($1.tokens)
		}
	}

	private func barHeight(_ tokens: UInt64, available: CGFloat) -> CGFloat {
		guard tokens > 0 else {
			return 1
		}

		let normalized = sqrt(Double(tokens) / Double(peak))
		return max(2, available * CGFloat(normalized))
	}

	private func barColor(_ tokens: UInt64) -> Color {
		let normalized = sqrt(Double(tokens) / Double(peak))
		return PanelPalette.usageCyan(colorScheme)
			.opacity(0.3 + 0.62 * normalized)
	}
}
