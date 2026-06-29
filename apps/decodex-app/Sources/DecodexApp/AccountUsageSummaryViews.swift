import Foundation
import SwiftUI

struct AccountUsageSummaryView: View {
	let account: CodexAccount
	let currentTime: Date

	var body: some View {
		VStack(spacing: 5) {
			if account.hasProfileSummary {
				AccountProfileSummaryView(account: account)
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
					currentTime: currentTime
				)
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
					currentTime: currentTime
				)
			}
		}
		.frame(maxWidth: .infinity)
		.padding(.horizontal, 1)
		.padding(.vertical, 1)
	}
}

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

struct AccountUsageMeterView: View {
	let label: String
	let remainingPercent: Int?
	let resetAtUnixEpoch: Int?
	let dailyAveragePercent: Double?
	let tone: AccountTone
	let currentTime: Date
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 3) {
			HStack(spacing: 5) {
				Text(label)
					.font(PanelFont.usageLabel)
					.frame(width: 28, alignment: .leading)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))

				Text(remainingText)
					.font(PanelFont.usageValue)
					.frame(width: 62, alignment: .leading)
					.foregroundStyle(valueColor)
					.monospacedDigit()

				if let dailyAverageText {
					HStack(alignment: .firstTextBaseline, spacing: 3) {
						Text("avg")
							.font(PanelFont.usageLabel)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
							.lineLimit(1)

						Text(dailyAverageText)
							.font(PanelFont.usageValue)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))
							.monospacedDigit()
							.lineLimit(1)
							.minimumScaleFactor(0.78)
					}
					.layoutPriority(1)
				}

				Spacer(minLength: 2)

				Text(resetDisplay.short)
					.font(PanelFont.usageValue)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.82 : 0.9))
					.monospacedDigit()
					.lineLimit(1)

				if resetDisplay.date.isEmpty == false {
					Text(resetDisplay.date)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.68 : 0.78))
						.lineLimit(1)
						.truncationMode(.middle)
					}
			}
			.frame(height: 14)

			GeometryReader { proxy in
				ZStack(alignment: .leading) {
					let width = fillWidth(in: proxy.size.width)

					Capsule()
						.fill(trackColor)
						.overlay {
							Capsule()
								.fill(trackInsetStyle)
								.padding(.vertical, 0.8)
								.allowsHitTesting(false)
						}
					Capsule()
						.fill(fillStyle)
						.frame(width: width)
						.clipShape(Capsule())
						.animation(PanelMotion.state, value: remainingPercent)
						.shadow(
							color: color.opacity(colorScheme == .dark ? 0.09 : 0.07),
							radius: colorScheme == .dark ? 1.2 : 1,
							x: 0,
							y: 0
						)
					Capsule()
						.strokeBorder(trackEdgeColor, lineWidth: 0.24)
						.allowsHitTesting(false)
				}
			}
			.frame(height: 3.2)
		}
		.lineLimit(1)
		.frame(height: 22)
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityText)
	}

	private var remainingText: String {
		guard let remainingPercent else {
			return "-"
		}

		return "\(remainingPercent)% left"
	}

	private var dailyAverageText: String? {
		guard let dailyAveragePercent else {
			return nil
		}
		let formatted = formatDailyUsageRate(dailyAveragePercent)

		return formatted == "-" ? nil : formatted
	}

	private var accessibilityText: String {
		let average = dailyAverageText.map { ", daily average \($0)" } ?? ""
		return "\(label) remaining \(remainingText)\(average), \(resetDisplay.accessibility)"
	}

	private var progress: CGFloat {
		guard let remainingPercent else {
			return 0
		}

		return CGFloat(max(0, min(100, remainingPercent))) / 100
	}

	private func fillWidth(in width: CGFloat) -> CGFloat {
		guard remainingPercent != nil else {
			return 0
		}

		return max(4, width * progress)
	}

	private var color: Color {
		switch tone {
		case .codexActive: return PanelPalette.codexAccent(colorScheme)
		case .ready: return PanelPalette.capacityAccent(colorScheme)
		case .selected: return PanelPalette.routeAccent(colorScheme)
		case .warning: return PanelPalette.warning(colorScheme)
		case .danger: return PanelPalette.destructive(colorScheme)
		case .neutral: return PanelPalette.secondaryText(colorScheme)
		}
	}

	private var valueColor: Color {
		switch tone {
		case .warning, .danger:
			return color.opacity(colorScheme == .dark ? 0.95 : 0.78)
		default:
			return PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.9 : 0.84)
		}
	}

	private var resetDisplay: UsageResetDisplay {
		UsageResetDisplay.make(resetAtUnixEpoch: resetAtUnixEpoch, now: currentTime)
	}

	private var trackColor: Color {
		PanelPalette.progressTrack(colorScheme)
	}

	private var trackEdgeColor: Color {
		PanelPalette.progressEdge(colorScheme)
	}

	private var fillStyle: LinearGradient {
		LinearGradient(
			colors: [
				color.opacity(colorScheme == .dark ? 0.78 : 0.68),
				color.opacity(colorScheme == .dark ? 0.62 : 0.52),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	private var trackInsetStyle: LinearGradient {
		LinearGradient(
			colors: [
				Color.white.opacity(colorScheme == .dark ? 0.022 : 0.05),
				Color.white.opacity(0),
				Color.black.opacity(colorScheme == .dark ? 0.035 : 0.018),
			],
			startPoint: .top,
			endPoint: .bottom
		)
	}
}

private struct UsageGlassTrackView: View {
	let progress: CGFloat
	let tint: Color
	let markers: [CGFloat]
	let alertMarker: CGFloat?
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		GeometryReader { proxy in
			let trackWidth = proxy.size.width
			let boundedProgress = max(0, min(1, progress))
			let fillWidth = max(boundedProgress > 0 ? 5 : 0, trackWidth * boundedProgress)

			ZStack(alignment: .leading) {
				Capsule()
					.fill(trackFill)
					.overlay {
						Capsule()
							.strokeBorder(PanelPalette.progressEdge(colorScheme), lineWidth: 0.4)
					}

				Capsule()
					.fill(fillFill)
					.frame(width: fillWidth)
					.shadow(color: tint.opacity(colorScheme == .dark ? 0.18 : 0.12), radius: 2, x: 0, y: 0)
					.animation(PanelMotion.state, value: progress)

				ForEach(markers, id: \.self) { marker in
					Rectangle()
						.fill(markerColor)
						.frame(width: 1.2)
						.padding(.vertical, 0.8)
						.offset(x: markerOffset(marker, in: trackWidth))
				}

				if let alertMarker {
					Rectangle()
						.fill(PanelPalette.destructive(colorScheme))
						.frame(width: 2)
						.padding(.vertical, 0.2)
						.offset(x: markerOffset(alertMarker, in: trackWidth))
				}
			}
		}
		.accessibilityHidden(true)
	}

	private var trackFill: LinearGradient {
		LinearGradient(
			colors: [
				PanelPalette.progressTrack(colorScheme).opacity(0.92),
				PanelPalette.progressTrack(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.82),
			],
			startPoint: .top,
			endPoint: .bottom
		)
	}

	private var fillFill: LinearGradient {
		LinearGradient(
			colors: [
				tint.opacity(colorScheme == .dark ? 0.9 : 0.78),
				tint.opacity(colorScheme == .dark ? 0.72 : 0.64),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	private var markerColor: Color {
		colorScheme == .dark
			? Color.white.opacity(0.48)
			: Color.white.opacity(0.76)
	}

	private func markerOffset(_ marker: CGFloat, in width: CGFloat) -> CGFloat {
		max(0, min(width - 1.2, width * max(0, min(1, marker))))
	}
}

struct UsageResetDisplay {
	let short: String
	let date: String
	let accessibility: String

	static func make(resetAtUnixEpoch: Int?, now: Date = Date()) -> UsageResetDisplay {
		guard let seconds = resetAtUnixEpoch, seconds > 0 else {
			return UsageResetDisplay(
				short: "-",
				date: "",
				accessibility: "reset unavailable"
			)
		}

		let resetAt = Date(timeIntervalSince1970: TimeInterval(seconds))
		guard resetAt.timeIntervalSince1970.isFinite else {
			return UsageResetDisplay(
				short: "unknown",
				date: "",
				accessibility: "remaining unknown"
			)
		}

		let distanceSeconds = Int(floor(resetAt.timeIntervalSince(now)))
		if distanceSeconds <= 0 {
			let date = formatResetDate(resetAt, now: now)
			return UsageResetDisplay(
				short: "0m",
				date: date,
				accessibility: "reset at \(date), reset due now"
			)
		}

		let short = formatResetDuration(distanceSeconds)
		let date = formatResetDate(resetAt, now: now)
		return UsageResetDisplay(
			short: short,
			date: date,
			accessibility: "reset at \(date), resets in \(short)"
		)
	}

	private static func formatResetDuration(_ seconds: Int) -> String {
		let value = max(0, seconds)
		if value < 60 {
			return "<1m"
		}

		let days = value / 86_400
		let hours = (value % 86_400) / 3_600
		let minutes = (value % 3_600) / 60

		if days > 0 {
			return hours > 0 ? "\(days)d \(hours)h" : "\(days)d"
		}

		if hours > 0 {
			return "\(hours)h \(minutes)m"
		}

		return "\(minutes)m"
	}

	private static func formatResetDate(_ date: Date, now: Date) -> String {
		let formatter = DateFormatter()
		formatter.locale = Locale(identifier: "en_US_POSIX")
		let calendar = Calendar(identifier: .gregorian)
		formatter.dateFormat = calendar.component(.year, from: date) == calendar.component(.year, from: now)
			? "MMM d HH:mm"
			: "MMM d yyyy HH:mm"
		return formatter.string(from: date)
	}
}

struct NoticeView: View {
	let text: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .top, spacing: 7) {
			Image(systemName: "exclamationmark.triangle")
				.foregroundStyle(PanelPalette.warning(colorScheme))
			Text(text)
				.font(PanelFont.notice)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
		}
		.padding(8)
		.modernGlassSurface(
			cornerRadius: 9,
			depth: .section
		)
	}
}

