import SwiftUI

enum PanelMotion {
	static let panelLayout = Animation.interactiveSpring(response: 0.3, dampingFraction: 0.92, blendDuration: 0.05)
}

extension View {
	func panelCardSurface(cornerRadius: CGFloat) -> some View {
		modifier(
			PanelCardSurfaceModifier(
				cornerRadius: cornerRadius
			)
		)
	}

	func panelModalSurface(cornerRadius: CGFloat) -> some View {
		modifier(
			PanelModalSurfaceModifier(
				cornerRadius: cornerRadius
			)
		)
	}
}

extension AnyTransition {
	static var panelSection: AnyTransition {
		.asymmetric(
			insertion: .opacity
				.combined(with: .offset(y: -4))
				.combined(with: .scale(scale: 0.992, anchor: .top)),
			removal: .opacity
				.combined(with: .offset(y: -3))
				.combined(with: .scale(scale: 0.996, anchor: .top))
		)
	}
}
