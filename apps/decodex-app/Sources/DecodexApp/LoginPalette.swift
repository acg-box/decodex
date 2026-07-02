import SwiftUI

enum LoginPalette {
	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.98, green: 0.985, blue: 1).opacity(0.99)
			: Color(red: 0.09, green: 0.11, blue: 0.15).opacity(0.96)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.86, green: 0.89, blue: 0.95).opacity(0.94)
			: Color(red: 0.28, green: 0.33, blue: 0.4).opacity(0.86)
	}

	static func accent(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.actionBlue(colorScheme)
	}

	static func warning(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.warning(colorScheme)
	}

	static func feedback(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.86, green: 0.93, blue: 1)
			: Color(red: 0.13, green: 0.32, blue: 0.52)
	}

	static func codeBoxFill(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.08, green: 0.1, blue: 0.14).opacity(0.72)
			: Color(red: 0.96, green: 0.975, blue: 1).opacity(0.92)
	}

	static func codeBoxStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.16)
			: Color(red: 0.48, green: 0.55, blue: 0.64).opacity(0.3)
	}

	static func codeBoxShadow(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.black.opacity(0.22)
			: Color(red: 0.24, green: 0.32, blue: 0.42).opacity(0.08)
	}
}
