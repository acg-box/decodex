import SwiftUI

enum PanelFont {
	private static func text(
		_ size: CGFloat,
		weight: Font.Weight,
		design: Font.Design = .default
	) -> Font {
		.system(size: size, weight: weight, design: design)
	}

	static let headerIcon = text(14.1, weight: .semibold)
	static let headerTitle = text(14.8, weight: .semibold)
	static let headerSubtitle = text(11.1, weight: .medium)

	static let emptyIcon = text(16.8, weight: .medium)
	static let emptyTitle = text(12.2, weight: .semibold)
	static let emptyBody = text(10.9, weight: .regular)
	static let notice = text(10.6, weight: .regular)

	static let accountName = text(13.1, weight: .semibold)
	static let accountDetail = text(10.9, weight: .medium)

	static let summaryIcon = text(10.4, weight: .medium)
	static let metricLabel = text(10.4, weight: .medium)
	static let metricValue = text(11.9, weight: .semibold)
	static let usageLabel = text(10.4, weight: .medium)
	static let usageValue = text(10.7, weight: .semibold)
	static let tertiary = text(9.7, weight: .medium)

	static let laneTitle = text(11.6, weight: .semibold)
	static let laneDetail = text(10.8, weight: .medium)
	static let laneStatus = text(10.6, weight: .medium)
	static let lanePopoverTitle = text(12.8, weight: .semibold)
	static let lanePopoverLabel = text(10.8, weight: .semibold)
	static let lanePopoverValue = text(11.0, weight: .semibold)
	static let lanePopoverMeta = text(10.6, weight: .medium)
	static let lanePopoverChip = text(10.5, weight: .semibold)

	static let iconButton = text(11.2, weight: .semibold)
}

enum LoginFont {
	private static func text(
		_ size: CGFloat,
		weight: Font.Weight,
		design: Font.Design = .default
	) -> Font {
		.system(size: size, weight: weight, design: design)
	}

	static let title = text(14.6, weight: .semibold)
	static let caption = text(10.6, weight: .medium)
	static let destination = text(10.8, weight: .semibold)
	static let button = text(10.8, weight: .semibold)
	static let icon = text(11.4, weight: .semibold)
	static let code = text(16.2, weight: .semibold, design: .monospaced)
}
