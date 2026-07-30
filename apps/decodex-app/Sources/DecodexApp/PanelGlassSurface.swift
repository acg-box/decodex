import SwiftUI

struct ModernGlassSurfaceModifier: ViewModifier {
	let cornerRadius: CGFloat

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		content
			.glassEffect(
				Glass.clear,
				in: shape
			)
	}
}
