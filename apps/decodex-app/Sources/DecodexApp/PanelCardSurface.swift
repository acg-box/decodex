import SwiftUI

enum PanelCardMaterial: String, CaseIterable, Identifiable, Sendable {
	static let storageKey = "decodex.operator.panelCardMaterial"

	case thin
	case liquidGlass = "liquid-glass"

	var id: Self { self }

	var title: String {
		switch self {
		case .thin:
			"Thin"
		case .liquidGlass:
			"Liquid Glass"
		}
	}
}

private struct PanelCardMaterialKey: EnvironmentKey {
	static let defaultValue = PanelCardMaterial.thin
}

extension EnvironmentValues {
	var panelCardMaterial: PanelCardMaterial {
		get { self[PanelCardMaterialKey.self] }
		set { self[PanelCardMaterialKey.self] = newValue }
	}
}

struct PanelCardSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.panelCardMaterial) private var panelCardMaterial
	let cornerRadius: CGFloat

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		switch panelCardMaterial {
		case .thin:
			content
				.background {
					shape.fill(.thinMaterial)
				}
				.shadow(
					color: Color.black.opacity(colorScheme == .dark ? 0.14 : 0.07),
					radius: 3,
					x: 0,
					y: 1
				)
		case .liquidGlass:
			content
				.glassEffect(.regular, in: shape)
		}
	}
}

struct PanelModalSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	let cornerRadius: CGFloat

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		content
			.background {
				shape.fill(.regularMaterial)
			}
			.shadow(
				color: Color.black.opacity(colorScheme == .dark ? 0.3 : 0.18),
				radius: 10,
				x: 0,
				y: 4
			)
	}
}
