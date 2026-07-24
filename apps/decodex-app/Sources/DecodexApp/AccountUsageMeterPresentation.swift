import Foundation
import SwiftUI

extension AccountUsageMeterView {
	var remainingText: String {
		guard let remainingPercent else {
			return "-"
		}

		return "\(remainingPercent)% left"
	}

	var dailyAverageText: String? {
		guard let dailyAveragePercent else {
			return nil
		}
		let formatted = formatDailyUsageRate(dailyAveragePercent)

		return formatted == "-" ? nil : formatted
	}

	var accessibilityText: String {
		let average = dailyAverageText.map { ", daily average \($0)" } ?? ""
		return "\(label) remaining \(remainingText)\(average), \(resetDisplay.accessibility)"
	}

	var progress: CGFloat {
		Self.normalizedProgress(for: remainingPercent)
	}

	static func normalizedProgress(for remainingPercent: Int?) -> CGFloat {
		guard let remainingPercent else {
			return 0
		}

		return CGFloat(max(0, min(100, remainingPercent))) / 100
	}

	static func shouldAnimateRefill(
		_ refillAnimation: AccountUsageMeterRefillAnimation?,
		to current: Int?
	) -> Bool {
		guard let refillAnimation, let current else {
			return false
		}

		return refillAnimation.fromPercent < 100 && current >= 100
	}

	func fillWidth(
		in width: CGFloat,
		progress: CGFloat
	) -> CGFloat {
		guard remainingPercent != nil else {
			return 0
		}

		return max(4, width * progress)
	}

	var color: Color {
		switch tone {
		case .codexActive: return PanelPalette.codexAccent(colorScheme)
		case .ready: return PanelPalette.capacityAccent(colorScheme)
		case .selected: return PanelPalette.routeAccent(colorScheme)
		case .warning: return PanelPalette.warning(colorScheme)
		case .danger: return PanelPalette.destructive(colorScheme)
		case .neutral: return PanelPalette.secondaryText(colorScheme)
		}
	}

	var valueColor: Color {
		switch tone {
		case .warning, .danger:
			return color.opacity(colorScheme == .dark ? 0.95 : 0.78)
		default:
			return PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.9 : 0.84)
		}
	}

	var resetDisplay: UsageResetDisplay {
		UsageResetDisplay.make(resetAtUnixEpoch: resetAtUnixEpoch, now: currentTime)
	}

	var trackColor: Color {
		PanelPalette.progressTrack(colorScheme)
	}

	var trackEdgeColor: Color {
		PanelPalette.progressEdge(colorScheme)
	}

	var fillStyle: LinearGradient {
		LinearGradient(
			colors: [
				color.opacity(colorScheme == .dark ? 0.78 : 0.68),
				color.opacity(colorScheme == .dark ? 0.62 : 0.52),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	var trackInsetStyle: LinearGradient {
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
