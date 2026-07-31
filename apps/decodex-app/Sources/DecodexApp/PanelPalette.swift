import SwiftUI

enum PanelPalette {
	static func primaryText(_: ColorScheme) -> Color {
		.primary
	}

	static func secondaryText(_: ColorScheme) -> Color {
		.secondary
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		.primary.opacity(colorScheme == .dark ? 0.085 : 0.11)
	}

	static func actionBlue(_: ColorScheme) -> Color {
		.accentColor
	}

	static func routeAccent(_: ColorScheme) -> Color {
		.accentColor
	}

	static func warning(_: ColorScheme) -> Color {
		.orange
	}

	static func usageCyan(_: ColorScheme) -> Color {
		.accentColor
	}

	static func fastModeAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark ? .yellow : .orange
	}

	static func destructive(_: ColorScheme) -> Color {
		.red
	}

	static func progressTrack(_ colorScheme: ColorScheme) -> Color {
		.secondary.opacity(colorScheme == .dark ? 0.2 : 0.16)
	}
}
