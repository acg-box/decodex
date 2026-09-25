import SwiftUI

struct AccountRouteActionPresentation: Equatable {
	let isCurrent: Bool
	let canSelect: Bool
	let canPerformDirectAccountControl: Bool
	let isAccountControlInProgress: Bool
	let isSubmittingResetCard: Bool

	var isDisabled: Bool {
		(isCurrent == false && canSelect == false)
			|| canPerformDirectAccountControl == false
			|| isAccountControlInProgress
			|| isSubmittingResetCard
	}

	var isVisuallyDisabled: Bool {
		isCurrent == false
			&& (
				canSelect == false
					|| canPerformDirectAccountControl == false
					|| isSubmittingResetCard
			)
	}

	var usesDisabledEnvironment: Bool {
		isVisuallyDisabled
	}

	func title(isSwitching: Bool) -> String {
		if isSwitching {
			return "Switching"
		}
		return isCurrent ? "Use automatic routing" : "Route through this account"
	}
}

struct AccountPrimaryActionsView: View {
	let state: ResetCardAccountState
	let store: ResetCardStore

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.compact) {
			CompactAccountActionButton(
				title: presentation.title(isSwitching: isSwitching),
				symbol: "arrow.triangle.branch",
				isActive: presentation.isCurrent,
				isDisabled: presentation.isDisabled,
				isVisuallyDisabled: presentation.isVisuallyDisabled,
				usesDisabledEnvironment: presentation.usesDisabledEnvironment,
				isBusy: store.isControllingAccount(
					state.account.accountID,
					activity: .route
				),
				help: presentation.isCurrent
					? "Pinned account · click to use automatic routing"
					: "Route through this account"
			) {
				Task {
					if presentation.isCurrent { await store.selectBalancedAccounts() }
					else { await store.routeAccount(state.account.accountID) }
				}
			}
		}
		.fixedSize(horizontal: true, vertical: false)
		.layoutPriority(1)
	}

	private var isCodexProjection: Bool {
		store.isCodexProjection(state.account.accountID)
	}

	private var isRouteCurrent: Bool {
		isCodexProjection && isFixed
	}

	private var presentation: AccountRouteActionPresentation {
		AccountRouteActionPresentation(
			isCurrent: isRouteCurrent,
			canSelect: canSelect,
			canPerformDirectAccountControl: store.canPerformDirectAccountControl,
			isAccountControlInProgress: store.isAccountControlInProgress,
			isSubmittingResetCard: store.submittingKey != nil
		)
	}

	private var canSelect: Bool {
		state.routeCapability == .ready
	}

	private var isSwitching: Bool {
		store.isControllingAccount(state.account.accountID, activity: .route)
	}

	private var isFixed: Bool {
		guard case .fixed(let accountID) = store.routing?.mode else {
			return false
		}
		return accountID == state.account.accountID
	}
}

struct AccountUtilityActionsView: View {
	let state: ResetCardAccountState
	let store: ResetCardStore
		@Environment(\.colorScheme) private var colorScheme
	@State private var isLogoutArmed = false

	var body: some View {
		CompactAccountActionButton(
			title: "Log out", symbol: "rectangle.portrait.and.arrow.right",
			isActive: false, isDisabled: lifecycleActionIsDisabled,
			isVisuallyDisabled: !store.canPerformDirectAccountControl,
			usesDisabledEnvironment: !store.canPerformDirectAccountControl,
			isBusy: false, help: "Log out"
		) { isLogoutArmed = true }
		.popover(isPresented: $isLogoutArmed, arrowEdge: .trailing) { logoutConfirmation }
		.onChange(of: state.account.accountRevision) { isLogoutArmed = false }
	}

	private var logoutConfirmation: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.section) {
			Text("Log out this account?")
				.font(PanelFont.transientTitle)

			Text("The account and its saved credential binding will be removed from Decodex.")
				.font(PanelFont.transientBody)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)

			HStack {
				Button("Cancel") {
					isLogoutArmed = false
				}
				.keyboardShortcut(.cancelAction)

				Spacer()

				Button("Log out", role: .destructive) {
					Task {
						await store.logoutAccount(state.account.accountID)
						isLogoutArmed = false
					}
				}
				.keyboardShortcut(.defaultAction)
				.disabled(lifecycleActionIsDisabled)
			}
		}
		.frame(width: 240)
		.padding(PanelSpacing.popoverInset)
	}

	private var lifecycleActionIsDisabled: Bool {
		store.canPerformDirectAccountControl == false
			|| store.isAccountControlInProgress
			|| store.submittingKey != nil
	}
}

struct AccountRefreshLoginButton: View {
	let state: ResetCardAccountState
	let store: ResetCardStore

	var body: some View {
		CompactAccountActionButton(
			title: "Refresh login",
			symbol: "person.crop.circle.badge.plus",
			isActive: false,
			isDisabled: isDisabled,
			isVisuallyDisabled: isDisabled,
			usesDisabledEnvironment: isDisabled,
			isBusy: store.isControllingAccount(
				state.account.accountID,
				activity: .loginRefresh
			),
			help: state.loginRefreshRecoveryOperationID == nil
				? "Sign in to this account with the official Codex device login."
				: "Sign in again to safely replace an uncertain account update."
		) {
			store.beginAccountReauthentication(for: state.account.accountID)
		}
	}

	private var isDisabled: Bool {
		state.account.credentialBinding == nil
			|| store.canPerformDirectAccountControl == false
			|| store.isControllingAccount(state.account.accountID)
			|| store.isEnrollingAccount
			|| store.isRoutingAccountControl
			|| store.accountReauthentication != nil
			|| store.submittingKey != nil
	}
}

private struct CompactAccountActionButton: View {
	let title: String
	let symbol: String
	let isActive: Bool
	let isDisabled: Bool
	let isVisuallyDisabled: Bool
	let usesDisabledEnvironment: Bool
	let isBusy: Bool
	let help: String
	let action: () -> Void
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme

	@State private var hovered = false
	var body: some View {
		Button {
			guard !isDisabled else { return }
			action()
		} label: {
			Image(systemName: symbol)
				.font(.system(size: 12, weight: isActive ? .semibold : .regular))
				.symbolVariant(isActive ? .fill : .none)
				.symbolRenderingMode(.hierarchical)
				.foregroundStyle(isActive ? PanelPalette.routeAccent(colorScheme) : PanelPalette.secondaryText(colorScheme))
				.frame(width: 24, height: 24)
				.opacity(hovered ? 1 : 0.85)
				.contentShape(RoundedRectangle(cornerRadius: 7))
				.symbolEffect(.pulse, options: .repeating, isActive: isBusy && !reduceMotion)
		}
		.buttonStyle(PanelPressButtonStyle(pressedScale: 0.94))
		.frame(width: 24, height: 24)
		.disabled(usesDisabledEnvironment)
		.allowsHitTesting(!isDisabled)
		.opacity(isVisuallyDisabled && !isActive ? 0.44 : 1)
		.onHover { hovered = $0 }
		.animation(controlStateAnimation, value: isActive)
		.animation(controlStateAnimation, value: hovered)
		.help(help)
		.accessibilityLabel(title)
		.accessibilityValue(isBusy ? "In progress" : (isActive ? "Selected" : ""))
	}

	private var controlStateAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.controlState
	}
}

struct AccountPowerButton: View {
	let state: ResetCardAccountState
	let store: ResetCardStore
	var body: some View {
		let enabled = state.account.enabled
		let unavailable = !store.canPerformDirectAccountControl
		CompactAccountActionButton(
			title: enabled ? "Disable account" : "Enable account", symbol: "power",
			isActive: enabled,
			isDisabled: unavailable || store.isAccountControlInProgress || store.submittingKey != nil,
			isVisuallyDisabled: unavailable, usesDisabledEnvironment: unavailable,
			isBusy: store.isControllingAccount(state.account.accountID, activity: .lifecycle),
			help: enabled ? "Disable account" : "Enable account"
		) { Task { await store.setAccount(state.account.accountID, enabled: !enabled) } }
		.accessibilityValue(enabled ? "Enabled" : "Disabled")
	}
}
