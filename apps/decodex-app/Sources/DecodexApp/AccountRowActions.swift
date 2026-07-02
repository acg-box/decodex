import SwiftUI

enum AccountActionClusterLayout {
	static let defaultWidth: CGFloat = 70
	static let confirmWidth: CGFloat = 82
	static let transition = Animation.interactiveSpring(response: 0.2, dampingFraction: 0.9, blendDuration: 0.03)
}

struct AccountDefaultActionsView: View {
	let account: CodexAccount
	let loginHelp: String
	let routeHelp: String
	let login: () -> Void
	let useInCodex: () -> Void
	let routeRunsHere: () -> Void
	let logout: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 3) {
			if account.needsLogin {
				PanelIconButtonView(
					symbol: "person.crop.circle.badge.plus",
					tint: PanelPalette.warning(colorScheme),
					isActive: false,
					isPrimary: true,
					size: 21,
					action: login,
					help: loginHelp
				)
			} else {
				PanelIconButtonView(
					symbol: account.codexActive ? "person.crop.circle.fill" : "person.crop.circle",
					tint: PanelPalette.codexAccent(colorScheme),
					isActive: account.codexActive,
					isDisabled: account.codexActive || account.canUseInCodex == false,
					isSubtle: true,
					size: 21,
					action: useInCodex,
					help: account.codexActive ? "Current Codex account" : "Use as Codex account"
				)
			}

			PanelIconButtonView(
				symbol: "arrow.triangle.branch",
				tint: account.selected
					? PanelPalette.routeAccent(colorScheme)
					: PanelPalette.actionBlue(colorScheme),
				isActive: account.selected,
				isDisabled: account.canRouteRuns == false && account.selected == false,
				isSubtle: true,
				size: 21,
				action: routeRunsHere,
				help: routeHelp
			)

			PanelIconButtonView(
				symbol: "trash",
				tint: PanelPalette.destructive(colorScheme),
				isActive: false,
				isDestructive: true,
				isSubtle: true,
				size: 21,
				action: logout,
				help: "Remove account"
			)
		}
	}
}

struct AccountLogoutConfirmActionsView: View {
	let displayName: String
	let errorMessage: String?
	let isPending: Bool
	let cancel: () -> Void
	let remove: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 5) {
			Button(action: cancel) {
				Image(systemName: "xmark")
					.font(PanelFont.tertiary)
					.frame(width: 15, height: 18)
			}
			.buttonStyle(AccountInlineConfirmButtonStyle(tint: PanelPalette.secondaryText(colorScheme)))
			.keyboardShortcut(.cancelAction)
			.disabled(isPending)
			.help("Keep \(displayName)")

			Button(role: .destructive, action: remove) {
				HStack(spacing: 3) {
					if isPending {
						ProgressView()
							.controlSize(.mini)
							.frame(width: 10, height: 10)
					} else {
						Image(systemName: "trash")
					}
					Text(isPending ? "Removing" : "Remove")
				}
				.font(PanelFont.tertiary)
				.lineLimit(1)
			}
			.buttonStyle(AccountInlineConfirmButtonStyle(tint: PanelPalette.destructive(colorScheme)))
			.disabled(isPending)
			.help("Remove \(displayName) from the local Decodex account pool")
		}
		.fixedSize(horizontal: true, vertical: false)
		.transition(.opacity)
	}
}

struct AccountInlineConfirmButtonStyle: ButtonStyle {
	let tint: Color

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.foregroundStyle(tint)
			.opacity(configuration.isPressed ? 0.72 : 1)
			.padding(.horizontal, 2)
			.padding(.vertical, 2)
			.contentShape(Rectangle())
			.scaleEffect(configuration.isPressed ? 0.97 : 1)
			.animation(PanelMotion.press, value: configuration.isPressed)
	}
}
