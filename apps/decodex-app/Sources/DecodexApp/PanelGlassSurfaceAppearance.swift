import SwiftUI

extension ModernGlassSurfaceModifier {
	var glassTint: Color? {
		switch depth {
		case .panel:
			return colorScheme == .dark
				? Color(red: 0.08, green: 0.1, blue: 0.14).opacity(0.18)
				: Color.white.opacity(0.05)
		case .section:
			return colorScheme == .dark
				? Color(red: 0.13, green: 0.16, blue: 0.22).opacity(0.18)
				: Color.white.opacity(0.1)
		case .row:
			return colorScheme == .dark
				? Color(red: 0.11, green: 0.14, blue: 0.19).opacity(0.18)
				: Color.white.opacity(0.08)
		case .control:
			return colorScheme == .dark
				? Color(red: 0.16, green: 0.19, blue: 0.25).opacity(0.22)
				: Color.white.opacity(0.13)
		}
	}

	var materialStyle: AnyShapeStyle {
		switch depth {
		case .panel:
			return AnyShapeStyle(.ultraThinMaterial)
		case .section:
			return AnyShapeStyle(.thinMaterial)
		case .row:
			return colorScheme == .dark ? AnyShapeStyle(.thinMaterial) : AnyShapeStyle(.ultraThinMaterial)
		case .control:
			return colorScheme == .dark ? AnyShapeStyle(.thinMaterial) : AnyShapeStyle(.ultraThinMaterial)
		}
	}

	var surfaceFill: Color {
		switch depth {
		case .panel:
			return colorScheme == .dark
				? Color(red: 0.04, green: 0.055, blue: 0.08).opacity(0.34)
				: Color(red: 0.95, green: 0.97, blue: 0.99).opacity(0.38)
		case .section:
			return colorScheme == .dark
				? Color(red: 0.12, green: 0.14, blue: 0.19).opacity(0.44)
				: Color(red: 0.8, green: 0.86, blue: 0.93).opacity(0.78)
		case .row:
			return colorScheme == .dark
				? Color(red: 0.095, green: 0.115, blue: 0.16).opacity(0.38)
				: Color(red: 0.82, green: 0.87, blue: 0.94).opacity(0.66)
		case .control:
			return colorScheme == .dark
				? Color(red: 0.12, green: 0.145, blue: 0.2).opacity(0.48)
				: Color(red: 0.74, green: 0.81, blue: 0.9).opacity(0.78)
		}
	}

	var surfaceStroke: Color {
		switch depth {
		case .panel:
			return PanelPalette.glassStroke(colorScheme)
		case .section:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.94 : 0.86)
		case .row:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.66)
		case .control:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.68 : 0.64)
		}
	}

	var strokeWidth: CGFloat {
		switch depth {
		case .panel:
			return 0.8
		case .section:
			return 0.7
		case .row, .control:
			return 0.6
		}
	}

	var surfaceShadow: Color {
		switch depth {
		case .panel:
			return PanelPalette.glassInnerShadow(colorScheme)
		case .section:
			return PanelPalette.glassInnerShadow(colorScheme).opacity(0.72)
		case .row:
			return PanelPalette.glassInnerShadow(colorScheme).opacity(0.5)
		case .control:
			return PanelPalette.glassInnerShadow(colorScheme).opacity(0.34)
		}
	}

	var shadowRadius: CGFloat {
		switch depth {
		case .panel:
			return 18
		case .section:
			return 9
		case .row:
			return 5
		case .control:
			return 3
		}
	}

	var shadowY: CGFloat {
		switch depth {
		case .panel:
			return 10
		case .section:
			return 5
		case .row:
			return 2
		case .control:
			return 1
		}
	}
}
