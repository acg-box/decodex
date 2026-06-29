import SwiftUI

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

struct ModernGlassSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	let cornerRadius: CGFloat
	let depth: GlassSurfaceDepth

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
		let appearanceID = colorScheme == .dark ? "dark" : "light"

		if #available(macOS 26.0, *) {
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
		} else {
			content
				.background {
					shape.fill(materialStyle)
					shape.fill(surfaceFill)
				}
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
				.id(appearanceID)
		}
	}

	@available(macOS 26.0, *)
	private var configuredGlass: Glass {
		var glass = Glass.regular.tint(glassTint)
		if depth == .control {
			glass = glass.interactive()
		}

		return glass
	}

	private var glassTint: Color? {
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

	private var materialStyle: AnyShapeStyle {
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

	private var surfaceFill: Color {
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

	private var surfaceStroke: Color {
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

	private var strokeWidth: CGFloat {
		switch depth {
		case .panel:
			return 0.8
		case .section:
			return 0.7
		case .row, .control:
			return 0.6
		}
	}

	private var surfaceShadow: Color {
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

	private var shadowRadius: CGFloat {
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

	private var shadowY: CGFloat {
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
