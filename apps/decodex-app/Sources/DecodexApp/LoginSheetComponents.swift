import SwiftUI

struct LoginCodeBoxesView: View {
	let code: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 4) {
			ForEach(0..<boxCount, id: \.self) { index in
				if index == 4 {
					Spacer()
						.frame(width: 2)
				}

				Text(character(at: index))
					.font(LoginFont.code)
					.monospacedDigit()
					.foregroundStyle(code.isEmpty ? LoginPalette.secondaryText(colorScheme).opacity(0.42) : LoginPalette.primaryText(colorScheme))
					.frame(width: 23, height: 31)
					.background {
						RoundedRectangle(cornerRadius: 7, style: .continuous)
							.fill(LoginPalette.codeBoxFill(colorScheme))
					}
					.overlay {
						RoundedRectangle(cornerRadius: 7, style: .continuous)
							.strokeBorder(LoginPalette.codeBoxStroke(colorScheme), lineWidth: 0.75)
							.allowsHitTesting(false)
					}
					.shadow(
						color: LoginPalette.codeBoxShadow(colorScheme),
						radius: colorScheme == .dark ? 2.5 : 1.4,
						y: 0.7
					)
			}
		}
		.frame(maxWidth: .infinity, alignment: .center)
	}

	private var characters: [String] {
		code.map { String($0).uppercased() }
	}

	private var boxCount: Int {
		max(8, min(12, characters.count))
	}

	private func character(at index: Int) -> String {
		guard characters.indices.contains(index) else {
			return ""
		}

		return characters[index]
	}
}

struct LoginIconActionButton: View {
	let symbol: String
	let feedbackSymbol: String?
	let isFeedbackActive: Bool
	let isEnabled: Bool
	let action: () -> Void
	let help: String
	@State private var isHovered = false

	var body: some View {
		Button(action: action) {
			Image(systemName: isFeedbackActive ? (feedbackSymbol ?? symbol) : symbol)
				.frame(width: 23, height: 22)
		}
		.buttonStyle(
			LoginSmallButtonStyle(
				isProminent: isHovered,
				isFeedbackActive: isFeedbackActive
			)
		)
		.disabled(isEnabled == false)
		.onHover { hovering in
			withAnimation(PanelMotion.hover) {
				isHovered = hovering
			}
		}
		.help(help)
	}
}

struct LoginSmallButtonStyle: ButtonStyle {
	var isProminent = false
	var isFeedbackActive = false
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(LoginFont.icon)
			.foregroundStyle(foreground)
			.modernGlassSurface(cornerRadius: 8, depth: .control)
			.overlay {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.strokeBorder(stroke, lineWidth: 0.55)
					.allowsHitTesting(false)
			}
			.shadow(
				color: shadowColor,
				radius: isFeedbackActive ? 4 : (isProminent ? 2.4 : 0),
				y: isFeedbackActive ? 1.2 : (isProminent ? 0.8 : 0)
			)
			.scaleEffect(configuration.isPressed ? 0.91 : (isFeedbackActive ? 1.055 : 1))
			.opacity(isEnabled ? 1 : 0.38)
			.animation(PanelMotion.press, value: configuration.isPressed)
			.animation(PanelMotion.hover, value: isProminent)
			.animation(PanelMotion.state, value: isFeedbackActive)
	}

	private var foreground: Color {
		if isEnabled == false {
			return LoginPalette.secondaryText(colorScheme).opacity(0.62)
		}
		if isFeedbackActive {
			return LoginPalette.feedback(colorScheme)
		}
		if isProminent {
			return LoginPalette.accent(colorScheme).opacity(colorScheme == .dark ? 1 : 0.95)
		}
		return LoginPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.9 : 0.78)
	}

	private var stroke: Color {
		if isEnabled == false {
			return LoginPalette.secondaryText(colorScheme).opacity(0.08)
		}
		if isFeedbackActive {
			return LoginPalette.feedback(colorScheme).opacity(colorScheme == .dark ? 0.36 : 0.26)
		}
		if isProminent {
			return LoginPalette.accent(colorScheme).opacity(colorScheme == .dark ? 0.22 : 0.18)
		}
		return LoginPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.1 : 0.12)
	}

	private var shadowColor: Color {
		if isFeedbackActive {
			return LoginPalette.feedback(colorScheme).opacity(colorScheme == .dark ? 0.22 : 0.16)
		}
		if isProminent {
			return Color.black.opacity(colorScheme == .dark ? 0.22 : 0.1)
		}
		return .clear
	}
}

struct LoginTextButtonStyle: ButtonStyle {
	var isPrimary = false
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(LoginFont.button)
			.foregroundStyle(foreground)
			.padding(.horizontal, isPrimary ? 11 : 9)
			.frame(height: 24)
			.modernGlassSurface(cornerRadius: 12, depth: .control)
			.scaleEffect(configuration.isPressed ? 0.965 : 1)
			.opacity(isEnabled ? 1 : 0.64)
			.animation(.interactiveSpring(response: 0.16, dampingFraction: 0.78), value: configuration.isPressed)
	}

	private var foreground: Color {
		if isPrimary {
			return LoginPalette.accent(colorScheme)
		}

		return LoginPalette.secondaryText(colorScheme)
	}

}

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
