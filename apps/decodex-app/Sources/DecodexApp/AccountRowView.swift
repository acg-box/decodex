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
					.transition(.panelInline)
			}

			if account.hasUsageSummary {
				AccountUsageSummaryView(account: account)
					.transition(.panelInline)
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
		.animation(PanelMotion.inlineLayout, value: runs.map(\.id))
		.animation(PanelMotion.inlineLayout, value: account.hasUsageSummary)
		.animation(PanelMotion.inlineLayout, value: showsDivider)
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
		.animation(PanelMotion.inlineLayout, value: isLogoutPending)
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
