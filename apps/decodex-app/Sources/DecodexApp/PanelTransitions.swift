import SwiftUI

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

	static var accountRowRemoval: AnyTransition {
		.asymmetric(
			insertion: .opacity
				.combined(with: .offset(y: -3))
				.combined(with: .scale(scale: 0.992, anchor: .top)),
			removal: .opacity
				.combined(with: .offset(y: -5))
				.combined(with: .scale(scale: 0.985, anchor: .top))
		)
	}

	static var panelInline: AnyTransition {
		.asymmetric(
			insertion: .opacity.combined(with: .offset(y: -2)),
			removal: .opacity.combined(with: .offset(y: -2))
		)
	}
}
