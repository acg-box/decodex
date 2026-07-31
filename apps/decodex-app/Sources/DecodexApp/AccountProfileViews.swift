import SwiftUI

struct AccountProfileSummaryView: View {
	let profile: AccountProfileSnapshot
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.related) {
			AccountProfileMetricsView(metrics: metrics)

			if profile.dailyUsage.isEmpty == false {
				AccountDailyUsageChart(
					records: profile.dailyUsage
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
		VStack(alignment: .leading, spacing: PanelSpacing.section) {
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
				HStack(spacing: PanelSpacing.related) {
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

			if let quotaDiagnostic {
				Label("Some usage data is unavailable", systemImage: "info.circle")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.help(quotaDiagnostic)
			}
		}
		.frame(width: 270)
		.padding(PanelSpacing.popoverInset)
		.accessibilityElement(children: .contain)
	}

	private var quotaDiagnostic: String? {
		let diagnostics = [
			quotaDiagnostic(title: "5-hour", window: state.fiveHourQuota),
			quotaDiagnostic(title: "7-day", window: state.sevenDayQuota),
		]
		.compactMap { $0 }

		return diagnostics.isEmpty ? nil : diagnostics.joined(separator: ". ")
	}

	private func quotaDiagnostic(
		title: String,
		window: ResetCardQuotaWindow
	) -> String? {
		guard case .error(let error) = window.state,
			error != .unsupportedWindow
		else {
			return nil
		}

		return "\(title) usage: \(error.presentation)"
	}
}

struct AccountProfileOverviewView: View {
	let aggregate: AccountProfileAggregate

	var body: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.compact) {
			if aggregateMetrics.isEmpty == false {
				AccountProfileMetricsView(
					metrics: aggregateMetrics
				)
			}

			if aggregate.dailyUsage.isEmpty == false {
				AccountDailyUsageChart(
					records: aggregate.dailyUsage
				)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(.horizontal, PanelSpacing.micro)
		.accessibilityElement(children: .combine)
	}

	private var aggregateMetrics: [AccountProfileMetric] {
		AccountProfileMetric.makeOverview(
			lifetimeTokens: aggregate.lifetimeTokens,
			peakDailyTokens: aggregate.peakDailyTokens,
			longestTaskSeconds: aggregate.longestTaskSeconds,
			currentStreakDays: aggregate.currentStreakDays,
			longestStreakDays: aggregate.longestStreakDays
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

	static func makeOverview(
		lifetimeTokens: UInt64?,
		peakDailyTokens: UInt64?,
		longestTaskSeconds: UInt64?,
		currentStreakDays: UInt32?,
		longestStreakDays: UInt32?
	) -> [Self] {
		[
			Self(
				id: "tokens",
				label: "total",
				value: lifetimeTokens.map(formatCompactCount) ?? "—"
			),
			Self(
				id: "peak",
				label: "peak",
				value: peakDailyTokens.map(formatCompactCount) ?? "—"
			),
			Self(
				id: "streak",
				label: "streak",
				value: streak(current: currentStreakDays, longest: longestStreakDays) ?? "—"
			),
			Self(
				id: "task",
				label: "task",
				value: longestTaskSeconds.map(formatActivityDuration) ?? "—"
			),
		]
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
			HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.micro) {
				ForEach(metrics) { metric in
					HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.micro) {
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
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		let values = displayRecords
		let peak = max(1, values.map(\.tokens).max() ?? 1)
		let totalTokens = values.reduce(UInt64(0)) {
			$0.addingWithoutOverflow($1.tokens)
		}

		GeometryReader { proxy in
			ZStack(alignment: .bottom) {
				Rectangle()
					.fill(PanelPalette.separator(colorScheme))
					.frame(height: 0.5)

				HStack(alignment: .bottom, spacing: 1.5) {
					ForEach(values) { record in
						RoundedRectangle(cornerRadius: 1.5, style: .continuous)
							.fill(barColor(record.tokens, peak: peak))
							.frame(
								maxWidth: .infinity,
								minHeight: 1,
								maxHeight: barHeight(
									record.tokens,
									peak: peak,
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
		.frame(height: 16)
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

	private func barHeight(
		_ tokens: UInt64,
		peak: UInt64,
		available: CGFloat
	) -> CGFloat {
		guard tokens > 0 else {
			return 1
		}

		let normalized = sqrt(Double(tokens) / Double(peak))
		return max(2, available * CGFloat(normalized))
	}

	private func barColor(_ tokens: UInt64, peak: UInt64) -> Color {
		let normalized = sqrt(Double(tokens) / Double(peak))
		return PanelPalette.usageCyan(colorScheme)
			.opacity(0.3 + 0.62 * normalized)
	}
}
