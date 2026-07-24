import Foundation
import SwiftUI

struct AccountUsageSummaryView: View {
	let account: CodexAccount

	var body: some View {
		TimelineView(.periodic(from: Date(), by: 30)) { timeline in
			VStack(spacing: 5) {
				if account.hasProfileSummary {
					AccountProfileSummaryView(account: account)
						.transition(.panelInline)
				}

				if account.hasResetCreditsSummary {
					AccountResetCreditsSummaryView(account: account)
						.transition(.panelInline)
				}

				if account.hasPrimaryUsageData {
					AccountUsageMeterView(
						label: account.windowLabel(seconds: account.primaryWindowSeconds),
						remainingPercent: account.primaryRemainingPercent,
						resetAtUnixEpoch: account.primaryResetsAtUnixEpoch,
						dailyAveragePercent: account.sevenDayAveragePercent(
							forWindowSeconds: account.primaryWindowSeconds
						),
						tone: account.usageTone(remainingPercent: account.primaryRemainingPercent),
						currentTime: timeline.date
					)
					.transition(.panelInline)
				}

				if account.hasSecondaryUsageData {
					AccountUsageMeterView(
						label: account.windowLabel(seconds: account.secondaryWindowSeconds),
						remainingPercent: account.secondaryRemainingPercent,
						resetAtUnixEpoch: account.secondaryResetsAtUnixEpoch,
						dailyAveragePercent: account.sevenDayAveragePercent(
							forWindowSeconds: account.secondaryWindowSeconds
						),
						tone: account.usageTone(remainingPercent: account.secondaryRemainingPercent),
						currentTime: timeline.date
					)
					.transition(.panelInline)
				}
			}
			.frame(maxWidth: .infinity)
			.padding(.horizontal, 1)
			.padding(.vertical, 1)
			.animation(PanelMotion.inlineLayout, value: account.hasProfileSummary)
			.animation(PanelMotion.inlineLayout, value: account.hasResetCreditsSummary)
			.animation(PanelMotion.inlineLayout, value: account.hasPrimaryUsageData)
			.animation(PanelMotion.inlineLayout, value: account.hasSecondaryUsageData)
		}
	}
}

struct AccountResetCreditsSummaryView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .center, spacing: 5) {
			PanelMetricIconView(
				symbol: "arrow.counterclockwise.circle",
				tint: PanelPalette.routeAccent(colorScheme).opacity(0.84)
			)

			Text(summaryText)
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.86))
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)

			AccountResetCreditExpiryStripView(account: account)
		}
		.frame(minHeight: 18)
		.accessibilityLabel(accessibilityLabel)
	}

	private var summaryText: String {
		let count = account.visibleResetCreditCount ?? account.availableResetCredits.count
		return "\(count) reset"
	}

	private var accessibilityLabel: String {
		let dates = account.availableResetCredits
			.map { credit in
				"expires \(formatResetCreditDate(credit.expiresAtUnixEpoch))"
			}
			.joined(separator: ", ")

		return dates.isEmpty ? "reset cards \(summaryText)" : "reset cards \(summaryText), \(dates)"
	}

	private func resetCreditHelp(_ credit: AccountResetCredit) -> String {
		"Expires \(formatResetCreditDate(credit.expiresAtUnixEpoch))"
	}
}

struct AccountResetCreditExpiryStripView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme
	@State private var placementStore = AccountRunStripPlacementStore()
	@State private var scrollProxy = AccountRunStripScrollProxy()
	@State private var scrollMetrics = AccountRunStripMetrics()
	@State private var showsEdgeControls = false

	var body: some View {
		HStack(spacing: AccountRunStripLayout.edgeControlSpacing) {
			if showsEdgeControls {
				edgeButton(.backward)
					.transition(.panelInline)
			}

			AccountRunStripScrollView(
				placementStore: placementStore,
				scrollProxy: scrollProxy,
				onMetricsChange: { metrics in
					updateScrollMetrics(metrics)
				}
			) {
				HStack(spacing: 4) {
					ForEach(Array(account.availableResetCredits.enumerated()), id: \.offset) { index, credit in
						resetCreditChip(formatResetCreditDate(credit.expiresAtUnixEpoch))
							.modifier(
								AccountRunChipPlacementReporter(
									runID: "reset-\(index)",
									placementStore: placementStore
								)
							)
							.help(resetCreditHelp(credit))
					}
				}
				.padding(.trailing, 1)
				.fixedSize(horizontal: true, vertical: false)
				.coordinateSpace(name: AccountRunStripLayout.contentCoordinateSpace)
			}
			.mask {
				AccountRunStripFadeMask(metrics: showsEdgeControls ? scrollMetrics : AccountRunStripMetrics())
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			if showsEdgeControls {
				edgeButton(.forward)
					.transition(.panelInline)
			}
		}
		.frame(height: AccountRunChipLayout.height)
		.frame(maxWidth: .infinity, alignment: .leading)
		.onAppear {
			placementStore.retainOnly(resetCreditIDs)
		}
		.onChange(of: resetCreditIDs) { _, ids in
			placementStore.retainOnly(ids)
		}
		.animation(PanelMotion.inlineLayout, value: showsEdgeControls)
	}

	private var resetCreditIDs: Set<String> {
		Set(account.availableResetCredits.indices.map { "reset-\($0)" })
	}

	private func edgeButton(_ direction: AccountRunStripScrollDirection) -> some View {
		AccountRunStripEdgeButton(
			direction: direction,
			isEnabled: direction == .backward
				? scrollMetrics.canScrollBackward
				: scrollMetrics.canScrollForward,
			accessibilityLabel: direction == .backward ? "Previous reset card" : "Next reset card",
			disabledHelp: direction == .backward
				? "Already at the first reset card"
				: "Already at the last reset card"
		) {
			scrollProxy.scrollToAdjacentRun(direction)
		} startContinuousAction: {
			scrollProxy.startContinuousScroll(direction)
		} stopContinuousAction: {
			scrollProxy.stopContinuousScroll()
		}
	}

	private func resetCreditChip(_ text: String) -> some View {
		Text(text)
			.font(PanelFont.usageValue)
			.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.88))
			.monospacedDigit()
			.lineLimit(1)
			.fixedSize(horizontal: true, vertical: false)
			.padding(.horizontal, 4)
			.padding(.vertical, 1)
			.background(resetCreditChipBackground)
	}

	private var resetCreditChipBackground: some ShapeStyle {
		PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.16 : 0.11)
	}

	private func resetCreditHelp(_ credit: AccountResetCredit) -> String {
		"Expires \(formatResetCreditDate(credit.expiresAtUnixEpoch))"
	}

	private func updateScrollMetrics(_ metrics: AccountRunStripMetrics) {
		let nextShowsEdgeControls = shouldShowEdgeControls(for: metrics)
		guard metrics != scrollMetrics || nextShowsEdgeControls != showsEdgeControls else {
			return
		}

		if showsEdgeControls && nextShowsEdgeControls == false {
			scrollProxy.stopContinuousScroll()
		}

		if metrics != scrollMetrics {
			var transaction = Transaction()
			transaction.disablesAnimations = true
			withTransaction(transaction) {
				scrollMetrics = metrics
			}
		}

		if nextShowsEdgeControls != showsEdgeControls {
			withAnimation(PanelMotion.inlineLayout) {
				showsEdgeControls = nextShowsEdgeControls
			}
		}
	}

	private func shouldShowEdgeControls(for metrics: AccountRunStripMetrics) -> Bool {
		let reservedWidth = showsEdgeControls ? AccountRunStripLayout.edgeControlReservedWidth : 0
		let viewportWidthWithoutEdgeControls = metrics.viewportWidth + reservedWidth

		return metrics.contentWidth > viewportWidthWithoutEdgeControls + AccountRunStripLayout.overflowTolerance
	}
}

func formatResetCreditDate(
	_ seconds: Int?,
	timeZone: TimeZone = .autoupdatingCurrent
) -> String {
	guard let seconds, seconds > 0 else {
		return "-"
	}
	let date = Date(timeIntervalSince1970: TimeInterval(seconds))
	guard date.timeIntervalSince1970.isFinite else {
		return "-"
	}

	let formatter = DateFormatter()
	formatter.locale = Locale(identifier: "en_US_POSIX")
	formatter.timeZone = timeZone
	formatter.dateFormat = "MMM d HH:mm"
	return formatter.string(from: date)
}
