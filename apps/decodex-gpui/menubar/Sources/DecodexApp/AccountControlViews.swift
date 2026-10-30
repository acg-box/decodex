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
		HStack(alignment: .center, spacing: PanelSpacing.compact) {
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
	@State private var confirmation = SecondTapConfirmation()
	@State private var confirmationTrigger = 0

	var body: some View {
		CompactAccountActionButton(
			title: "Sign out", symbol: "rectangle.portrait.and.arrow.right",
			isActive: false, isDisabled: lifecycleActionIsDisabled,
			isVisuallyDisabled: !store.canPerformDirectAccountControl,
			usesDisabledEnvironment: !store.canPerformDirectAccountControl,
			isBusy: false, help: "Sign out",
			isConfirming: confirmation.isArmed(), confirmationTrigger: confirmationTrigger
		) {
			if confirmation.tap() {
				Task { await store.logoutAccount(state.account.accountID) }
			} else {
				confirmationTrigger += 1
			}
		}
		.task(id: confirmation.deadline) {
			guard let deadline = confirmation.deadline else { return }
			do { try await ContinuousClock().sleep(until: deadline) }
			catch { return }
			if confirmation.deadline == deadline { confirmation.cancel() }
		}
		.onChange(of: state.account.accountRevision) { confirmation.cancel() }
		.onChange(of: lifecycleActionIsDisabled) {
			if lifecycleActionIsDisabled { confirmation.cancel() }
		}
		.onDisappear { confirmation.cancel() }
		.onExitCommand(perform: confirmation.isArmed() ? { confirmation.cancel() } : nil)
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
			title: "Sign in",
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
				? "Sign in with a one-time code."
				: "Sign in again to reconnect."
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
	var isConfirming = false
	var confirmationTrigger = 0
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
				.font(.system(size: 11, weight: isActive ? .semibold : .regular))
				.symbolVariant(isActive ? .fill : .none)
				.symbolRenderingMode(.hierarchical)
				.foregroundStyle(isConfirming ? PanelPalette.destructive(colorScheme) : (isActive ? PanelPalette.routeAccent(colorScheme) : PanelPalette.secondaryText(colorScheme)))
				.frame(width: 20, height: 20)
				.opacity(hovered ? 1 : 0.85)
				.contentShape(RoundedRectangle(cornerRadius: 7))
				.symbolEffect(.pulse, options: .repeating, isActive: isBusy && !reduceMotion)
				.keyframeAnimator(initialValue: CGFloat.zero, trigger: confirmationTrigger) { [reduceMotion] content, offset in
					content.offset(x: reduceMotion ? 0 : offset)
				} keyframes: { _ in
					LinearKeyframe(-3, duration: 0.04)
					LinearKeyframe(3, duration: 0.06)
					LinearKeyframe(-2, duration: 0.05)
					LinearKeyframe(1, duration: 0.05)
					LinearKeyframe(0, duration: 0.04)
				}
		}
		.buttonStyle(PanelPressButtonStyle(pressedScale: 0.94))
		.frame(width: 20, height: 20)
		.disabled(usesDisabledEnvironment)
		.allowsHitTesting(!isDisabled)
		.opacity(isVisuallyDisabled && !isActive ? 0.44 : 1)
		.onHover { hovered = $0 }
		.animation(controlStateAnimation, value: isActive)
		.animation(controlStateAnimation, value: isConfirming)
		.animation(controlStateAnimation, value: hovered)
		.help(help)
		.accessibilityLabel(title)
		.accessibilityValue(isConfirming ? "Confirmation required" : (isBusy ? "In progress" : (isActive ? "Selected" : "")))
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
