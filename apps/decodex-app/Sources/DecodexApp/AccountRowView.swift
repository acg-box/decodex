import SwiftUI

struct AccountRowView: View {
	let account: CodexAccount
	let runs: [OperatorCurrentLaneCard]
	let displayName: String
	let showsDivider: Bool
	let isLogoutArmed: Bool
	let currentTime: Date
	let useInCodex: () -> Void
	let routeRunsHere: () -> Void
	let login: () -> Void
	let logout: () -> Void
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
						symbol: isLogoutArmed ? "trash.fill" : "trash",
						tint: PanelPalette.destructive(colorScheme),
						isActive: isLogoutArmed,
						isDestructive: true,
						isSubtle: isLogoutArmed == false,
						size: 21,
						action: logout,
						help: isLogoutArmed ? "Click again to confirm removal" : "Remove account"
					)
					.modifier(DeleteArmedShakeModifier(isArmed: isLogoutArmed))
				}
			}

			if runs.isEmpty == false {
				AccountRunSummaryView(runs: runs, currentTime: currentTime)
			}

			if account.hasUsageSummary {
				AccountUsageSummaryView(account: account, currentTime: currentTime)
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
		.animation(PanelMotion.state, value: isLogoutArmed)
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

private struct DeleteArmedShakeModifier: ViewModifier {
	let isArmed: Bool
	@State private var shakeTrigger = 0

	func body(content: Content) -> some View {
		content
			.modifier(DeleteShakeEffect(animatableData: CGFloat(shakeTrigger)))
			.scaleEffect(isArmed ? 1.045 : 1)
			.onChange(of: isArmed) { _, armed in
				guard armed else {
					return
				}
				withAnimation(.linear(duration: 0.42)) {
					shakeTrigger += 1
				}
			}
	}
}

private struct DeleteShakeEffect: GeometryEffect {
	var travel: CGFloat = 1.8
	var shakesPerUnit: CGFloat = 3
	var animatableData: CGFloat

	func effectValue(size: CGSize) -> ProjectionTransform {
		let xOffset = travel * sin(animatableData * .pi * shakesPerUnit * 2)
		return ProjectionTransform(CGAffineTransform(translationX: xOffset, y: 0))
	}
}

