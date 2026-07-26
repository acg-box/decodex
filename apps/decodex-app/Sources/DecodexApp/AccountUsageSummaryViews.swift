import Foundation
import SwiftUI

struct AccountUsageSummaryView: View {
	let account: CodexAccount
	let usageRefillAnimation: AccountUsageRefillAnimation?

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
						currentTime: timeline.date,
						refillAnimation: usageRefillAnimation?.meterAnimation(
							for: .primary,
							currentPercent: account.primaryRemainingPercent
						)
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
						currentTime: timeline.date,
						refillAnimation: usageRefillAnimation?.meterAnimation(
							for: .secondary,
							currentPercent: account.secondaryRemainingPercent
						)
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
				.accessibilityHidden(true)

			Text(summaryText)
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.86))
				.lineLimit(1)
				.frame(maxWidth: .infinity, alignment: .leading)

			Text("Legacy · read only")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(minHeight: 18)
		.accessibilityElement(children: .ignore)
		.accessibilityLabel("Legacy reset cards, \(summaryText), read only")
		.help("Legacy account rows cannot be joined safely to vNext account IDs. Use reset cards only in the separate vNext section.")
	}

	private var summaryText: String {
		let count = account.visibleResetCreditCount ?? account.availableResetCredits.count
		return "\(count) reset card\(count == 1 ? "" : "s")"
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
