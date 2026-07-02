import SwiftUI

enum OperatorLaneReadoutLayout {
	static let titleWidth: CGFloat = 92
	static let columnSpacing: CGFloat = 7
	static let itemRowSpacing: CGFloat = 2
	static let progressTrackWidth: CGFloat = 84
	static let totalSegmentSpacing: CGFloat = 13
	static let totalValueLabelSpacing: CGFloat = 4
	static let metricColumnCount = 4
	static let lifecycleTableColumnSpacing: CGFloat = 18
}

enum OperatorLanePopoverStyle {
	static let titleFont = PanelFont.laneTitle
	static let projectFont = PanelFont.laneDetail
	static let labelFont = PanelFont.lanePopoverLabel
	static let valueFont = PanelFont.lanePopoverValue
	static let metaFont = PanelFont.lanePopoverMeta
	static let separatorFont = PanelFont.lanePopoverMeta

	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		Color.primary.opacity(colorScheme == .dark ? 0.94 : 0.88)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.92 : 0.86)
	}

	static func mutedText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.78 : 0.68)
	}

	static func tableHeaderText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.72 : 0.62)
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.18 : 0.22)
	}

	static func progressTrack(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.2 : 0.22)
	}

	static func progressFill(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.86 : 0.76)
	}
}
