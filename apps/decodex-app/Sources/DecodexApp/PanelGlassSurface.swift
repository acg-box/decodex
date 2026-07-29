import SwiftUI

struct ModernGlassSurfaceModifier: ViewModifier {
	let cornerRadius: CGFloat
	let depth: GlassSurfaceDepth

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		content
			.glassEffect(
				configuredGlass,
				in: shape
			)
	}

	var configuredGlass: Glass {
		var glass = Glass.clear
		if depth == .control {
			glass = glass.interactive()
		}

		return glass
	}
}
