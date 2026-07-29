import SwiftUI

enum PanelPalette {
	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.95, green: 0.96, blue: 0.98).opacity(0.97)
			: Color(red: 0.12, green: 0.14, blue: 0.18).opacity(0.94)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.73, green: 0.76, blue: 0.82).opacity(0.84)
			: Color(red: 0.34, green: 0.38, blue: 0.45).opacity(0.8)
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.065)
			: Color(red: 0.32, green: 0.38, blue: 0.46).opacity(0.14)
	}

	static func actionBlue(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.86, green: 0.89, blue: 0.94)
			: Color(red: 0.18, green: 0.29, blue: 0.4)
	}

	static func routeAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.68, green: 0.8, blue: 0.96)
			: Color(red: 0.13, green: 0.32, blue: 0.56)
	}

	static func warning(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.95, green: 0.68, blue: 0.38)
			: Color(red: 0.62, green: 0.4, blue: 0.12)
	}

	static func usageCyan(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.35, green: 0.78, blue: 0.86)
			: Color(red: 0.1, green: 0.53, blue: 0.62)
	}

	static func fastModeAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.98, green: 0.84, blue: 0.48)
			: Color(red: 0.42, green: 0.31, blue: 0.09)
	}

	static func destructive(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.98, green: 0.4, blue: 0.45)
			: Color(red: 0.68, green: 0.1, blue: 0.16)
	}

	static func progressTrack(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.09)
			: Color(red: 0.15, green: 0.23, blue: 0.3).opacity(0.1)
	}

}
