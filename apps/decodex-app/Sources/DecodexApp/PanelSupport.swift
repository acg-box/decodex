import Foundation
import SwiftUI

func panelTrimmed(_ value: String?) -> String? {
	value?.trimmingCharacters(in: .whitespacesAndNewlines)
}

struct PanelMetricIconView: View {
	let symbol: String
	let tint: Color

	var body: some View {
		Image(systemName: symbol)
			.font(PanelFont.summaryIcon)
			.symbolRenderingMode(.monochrome)
			.foregroundStyle(tint)
			.frame(width: 12, height: 12)
			.alignmentGuide(.firstTextBaseline) { dimensions in
				dimensions[VerticalAlignment.center] + 3.85
			}
	}
}

enum PanelMotion {
	static let hover = Animation.interactiveSpring(response: 0.22, dampingFraction: 0.86, blendDuration: 0.04)
	static let press = Animation.interactiveSpring(response: 0.16, dampingFraction: 0.78, blendDuration: 0.02)
	static let state = Animation.interactiveSpring(response: 0.24, dampingFraction: 0.88, blendDuration: 0.05)
	static let inlineLayout = Animation.interactiveSpring(response: 0.2, dampingFraction: 0.9, blendDuration: 0.03)
	static let panelLayout = Animation.interactiveSpring(response: 0.3, dampingFraction: 0.92, blendDuration: 0.05)
	static let accountRemoval = Animation.interactiveSpring(response: 0.28, dampingFraction: 0.94, blendDuration: 0.04)
	static let meterRefill = Animation.timingCurve(0.18, 0.82, 0.24, 1, duration: 0.72)
}

enum GlassSurfaceDepth {
	case panel
	case section
	case row
	case control
}

extension View {
	func modernGlassSurface(
		cornerRadius: CGFloat,
		depth: GlassSurfaceDepth = .section
	) -> some View {
		modifier(
			ModernGlassSurfaceModifier(
				cornerRadius: cornerRadius,
				depth: depth
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
