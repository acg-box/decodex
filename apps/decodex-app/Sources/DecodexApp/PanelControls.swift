import SwiftUI

struct PanelIconButtonView: View {
	let symbol: String
	let tint: Color
	let isActive: Bool
	let isDestructive: Bool
	let isDisabled: Bool
	let isSubtle: Bool
	let isPrimary: Bool
	let size: CGFloat
	let action: () -> Void
	let help: String
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme

	init(
		symbol: String,
		tint: Color,
		isActive: Bool,
		isDestructive: Bool = false,
		isDisabled: Bool = false,
		isSubtle: Bool = false,
		isPrimary: Bool = false,
		size: CGFloat = 24,
		action: @escaping () -> Void,
		help: String
	) {
		self.symbol = symbol
		self.tint = tint
		self.isActive = isActive
		self.isDestructive = isDestructive
		self.isDisabled = isDisabled
		self.isSubtle = isSubtle
		self.isPrimary = isPrimary
		self.size = size
		self.action = action
		self.help = help
	}

	var body: some View {
		Button(action: action) {
			buttonLabel
		}
		.buttonStyle(PanelPressButtonStyle())
		.disabled(isDisabled)
		.animation(controlStateAnimation, value: symbol)
		.animation(controlStateAnimation, value: isActive)
		.help(help)
		.accessibilityLabel(help)
	}

	private var buttonLabel: some View {
		iconContent
			.opacity(isDisabled && isActive == false ? 0.5 : 0.9)
	}

	private var iconContent: some View {
		Image(systemName: symbol)
			.font(PanelFont.iconButton)
			.symbolRenderingMode(.monochrome)
			.contentTransition(.symbolEffect(.replace))
			.foregroundStyle(foregroundColor)
			.frame(width: size, height: size)
			.contentShape(RoundedRectangle(cornerRadius: iconCornerRadius, style: .continuous))
	}

	private var controlStateAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.controlState
	}

	private var foregroundColor: Color {
		if isActive {
			return tint.opacity(colorScheme == .dark ? 0.98 : 0.92)
		}
		if isDisabled {
			return PanelPalette.secondaryText(colorScheme)
		}
		if isDestructive {
			return tint.opacity(colorScheme == .dark ? 0.96 : 0.9)
		}
		if isPrimary {
			return tint.opacity(colorScheme == .dark ? 1 : 0.96)
		}
		if isSubtle {
			return tint.opacity(colorScheme == .dark ? 0.86 : 0.82)
		}
		return PanelPalette.actionBlue(colorScheme).opacity(colorScheme == .dark ? 0.88 : 0.86)
	}

	private var iconCornerRadius: CGFloat {
		size * 0.5
	}
}

struct PanelPressButtonStyle: ButtonStyle {
	let pressedScale: CGFloat
	@Environment(\.accessibilityReduceMotion) private var reduceMotion

	init(pressedScale: CGFloat = 0.92) {
		self.pressedScale = pressedScale
	}

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.opacity(configuration.isPressed ? 0.7 : 1)
			.scaleEffect(configuration.isPressed ? pressedScale : 1)
			.animation(
				reduceMotion ? nil : PanelMotion.press,
				value: configuration.isPressed
			)
	}
}
