import AppKit
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
				shape.fill(cardFill)
			}
			.overlay {
				shape
					.strokeBorder(cardStroke, lineWidth: 0.7)
					.allowsHitTesting(false)
			}
			.shadow(
				color: Color.black.opacity(colorScheme == .dark ? 0.24 : 0.12),
				radius: 8,
				x: 0,
				y: 3
			)
	}

	private var cardFill: Color {
		colorScheme == .dark
			? Color(red: 0.2, green: 0.2, blue: 0.21).opacity(0.82)
			: Color(nsColor: .controlBackgroundColor).opacity(0.84)
	}

	private var cardStroke: Color {
		colorScheme == .dark
			? Color.white.opacity(0.07)
			: Color.black.opacity(0.1)
	}
}
