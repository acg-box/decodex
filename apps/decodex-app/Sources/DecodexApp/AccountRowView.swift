import SwiftUI

struct AccountRowView: View {
	let account: CodexAccount
	let runs: [OperatorCurrentLaneCard]
	let displayName: String
	let showsDivider: Bool
	let isLogoutArmed: Bool
	let isLogoutPending: Bool
	let logoutErrorMessage: String?
	let useInCodex: () -> Void
	let routeRunsHere: () -> Void
	let login: () -> Void
	let logout: () -> Void
	let cancelLogout: () -> Void
	let confirmLogout: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			HStack(alignment: .center, spacing: 8) {
				HStack(alignment: .firstTextBaseline, spacing: 5) {
					Text(displayName)
						.font(PanelFont.accountName)
						.foregroundStyle(PanelPalette.primaryText(colorScheme))
						.lineLimit(1)
						.truncationMode(.middle)
						.layoutPriority(1)

					if let capacityLabel = account.currentCapacityLabel {
						Text("·")
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.62))
							.fixedSize(horizontal: true, vertical: false)

						Text(capacityLabel)
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))
							.lineLimit(1)
							.fixedSize(horizontal: true, vertical: false)
					}

					if let healthLabel = account.compactHealthLabel {
						Text("·")
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.62))
							.fixedSize(horizontal: true, vertical: false)

						Text(healthLabel)
							.font(PanelFont.accountDetail)
							.foregroundStyle(account.statusDisplayColor(colorScheme: colorScheme))
							.lineLimit(1)
							.fixedSize(horizontal: true, vertical: false)
					}

				}
				.frame(maxWidth: .infinity, alignment: .leading)

				actionCluster
			}

			if runs.isEmpty == false {
				AccountRunSummaryView(runs: runs)
			}

			if account.hasUsageSummary {
				AccountUsageSummaryView(account: account)
			}
		}
		.padding(.vertical, 7)
		.padding(.leading, 8)
		.padding(.trailing, 7)
		.overlay(alignment: .bottom) {
			if showsDivider {
				Rectangle()
					.fill(PanelPalette.separator(colorScheme).opacity(colorScheme == .dark ? 0.48 : 0.72))
					.frame(height: 0.5)
					.padding(.leading, 8)
					.padding(.trailing, 7)
					.allowsHitTesting(false)
			}
		}
		.animation(PanelMotion.state, value: account.selected)
		.animation(PanelMotion.state, value: account.codexActive)
	}

	@ViewBuilder
	private var actionCluster: some View {
		ZStack(alignment: .trailing) {
			AccountDefaultActionsView(
				account: account,
				loginHelp: loginHelp,
				routeHelp: routeHelp,
				login: login,
				useInCodex: useInCodex,
				routeRunsHere: routeRunsHere,
				logout: logout
			)
			.opacity(isLogoutArmed ? 0 : 1)
			.offset(x: isLogoutArmed ? -5 : 0)
			.allowsHitTesting(isLogoutArmed == false)

			AccountLogoutConfirmActionsView(
				displayName: displayName,
				errorMessage: logoutErrorMessage,
				isPending: isLogoutPending,
				cancel: cancelLogout,
				remove: confirmLogout
			)
			.opacity(isLogoutArmed ? 1 : 0)
			.offset(x: isLogoutArmed ? 0 : 5)
			.allowsHitTesting(isLogoutArmed)
		}
		.frame(width: isLogoutArmed ? AccountActionClusterLayout.confirmWidth : AccountActionClusterLayout.defaultWidth, alignment: .trailing)
		.clipped()
		.animation(AccountActionClusterLayout.transition, value: isLogoutArmed)
	}

	private var routeHelp: String {
		if account.selected {
			return "Restore balanced run routing"
		}
		if account.needsLogin {
			return "Sign in again before routing runs"
		}
		if account.disabled {
			return "Disabled account cannot route runs"
		}

		return "Route Decodex runs here"
	}

	private var loginHelp: String {
		if account.recoveryActionKind == .login {
			return "Refresh token was rejected; sign in again"
		}

		return "Login account"
	}
}

private enum AccountActionClusterLayout {
	static let defaultWidth: CGFloat = 70
	static let confirmWidth: CGFloat = 82
	static let transition = Animation.interactiveSpring(response: 0.2, dampingFraction: 0.9, blendDuration: 0.03)
}

private struct AccountDefaultActionsView: View {
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

private struct AccountLogoutConfirmActionsView: View {
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

private struct AccountInlineConfirmButtonStyle: ButtonStyle {
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
