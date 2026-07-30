import SwiftUI

struct PanelCardSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	let cornerRadius: CGFloat

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		content
			.background {
				shape.fill(.thinMaterial)
			}
			.shadow(
				color: Color.black.opacity(colorScheme == .dark ? 0.14 : 0.07),
				radius: 3,
				x: 0,
				y: 1
			)
	}
}

struct PanelModalSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	let cornerRadius: CGFloat

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		content
			.background {
				shape.fill(.regularMaterial)
			}
			.shadow(
				color: Color.black.opacity(colorScheme == .dark ? 0.3 : 0.18),
				radius: 10,
				x: 0,
				y: 4
			)
	}
}
