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

				ScrollView(.horizontal, showsIndicators: false) {
					HStack(spacing: 3) {
						ForEach(Array(account.availableResetCredits.enumerated()), id: \.offset) { _, credit in
						Text(formatResetCreditDate(credit.expiresAtUnixEpoch))
							.font(PanelFont.usageValue)
							.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.88))
							.monospacedDigit()
							.lineLimit(1)
							.fixedSize(horizontal: true, vertical: false)
							.padding(.horizontal, 4)
							.padding(.vertical, 1)
							.background(resetCreditChipBackground)
							.help(resetCreditHelp(credit))
					}
				}
				.frame(maxWidth: .infinity, alignment: .leading)
			}
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.frame(minHeight: 18)
		.accessibilityLabel(accessibilityLabel)
	}

	private var summaryText: String {
		let count = account.visibleResetCreditCount ?? account.availableResetCredits.count
		return "\(count) reset"
	}

	private var resetCreditChipBackground: some ShapeStyle {
		PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.16 : 0.11)
	}

	private var accessibilityLabel: String {
		let dates = account.availableResetCredits
			.map { credit in
				"expires \(formatResetCreditDate(credit.expiresAtUnixEpoch)) BJT"
			}
			.joined(separator: ", ")

		return dates.isEmpty ? "reset cards \(summaryText) BJT" : "reset cards \(summaryText) BJT, \(dates)"
	}

	private func resetCreditHelp(_ credit: AccountResetCredit) -> String {
		"Expires \(formatResetCreditDate(credit.expiresAtUnixEpoch)) BJT"
	}
}

func formatResetCreditDate(_ seconds: Int?) -> String {
	guard let seconds, seconds > 0 else {
		return "-"
	}
	let date = Date(timeIntervalSince1970: TimeInterval(seconds))
	guard date.timeIntervalSince1970.isFinite else {
		return "-"
	}

	let formatter = DateFormatter()
	formatter.locale = Locale(identifier: "en_US_POSIX")
	formatter.timeZone = TimeZone(identifier: "Asia/Shanghai")
	formatter.dateFormat = "MMM d HH:mm"
	return formatter.string(from: date)
}
