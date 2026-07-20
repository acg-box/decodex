import SwiftUI

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
		.accessibilityLabel(help)
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
			.animation(PanelMotion.press, value: configuration.isPressed)
	}

	private var foreground: Color {
		if isPrimary {
			return LoginPalette.accent(colorScheme)
		}

		return LoginPalette.secondaryText(colorScheme)
	}
}
