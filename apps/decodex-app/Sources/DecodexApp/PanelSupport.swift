import SwiftUI

enum PanelSpacing {
	// Compact 2-point rhythm for the menu-bar panel.
	static let micro: CGFloat = 2
	static let compact: CGFloat = 4
	static let related: CGFloat = 6
	static let section: CGFloat = 8

	// All primary cards share the same content edge.
	static let cardHorizontal: CGFloat = 10
	static let cardVertical: CGFloat = 8

	// Popovers and floating modal content use one larger inset.
	static let popoverInset: CGFloat = 12
}

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
