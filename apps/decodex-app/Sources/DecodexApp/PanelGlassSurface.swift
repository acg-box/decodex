import SwiftUI

struct ModernGlassSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) var colorScheme
	let cornerRadius: CGFloat
	let depth: GlassSurfaceDepth

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
		let appearanceID = colorScheme == .dark ? "dark" : "light"

		content
			.background {
				shape.fill(surfaceFill)
			}
			.glassEffect(
				configuredGlass,
				in: shape
			)
			.overlay {
				shape
					.strokeBorder(surfaceStroke, lineWidth: strokeWidth)
					.allowsHitTesting(false)
			}
			.shadow(
				color: surfaceShadow,
				radius: shadowRadius,
				x: 0,
				y: shadowY
			)
			// Menu-bar glass layers can keep a stale material across system appearance flips.
			// Re-key only the surface wrapper so light/dark changes redraw immediately.
			.id(appearanceID)
	}

	var configuredGlass: Glass {
		var glass = Glass.regular.tint(glassTint)
		if depth == .control {
			glass = glass.interactive()
		}

		return glass
	}
}
