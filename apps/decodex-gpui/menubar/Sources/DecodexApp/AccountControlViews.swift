import SwiftUI

struct AccountRouteActionPresentation: Equatable {
	let isCurrent: Bool
	let canSelect: Bool
	let canPerformDirectAccountControl: Bool
	let isAccountControlInProgress: Bool
	let isSubmittingResetCard: Bool

	var isDisabled: Bool {
		isCurrent
			|| canSelect == false
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
		isCurrent || isVisuallyDisabled
	}
}

struct AccountPrimaryActionsView: View {
	let state: ResetCardAccountState
	let store: ResetCardStore

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.compact) {
			CompactAccountActionButton(
				title: presentation.isCurrent ? "Routed" : "Route",
				symbol: presentation.isCurrent
					? "point.3.connected.trianglepath.dotted"
					: "arrow.triangle.branch",
				isActive: presentation.isCurrent,
				isDisabled: presentation.isDisabled,
				isVisuallyDisabled: presentation.isVisuallyDisabled,
				usesDisabledEnvironment: presentation.usesDisabledEnvironment,
				isBusy: store.isControllingAccount(
					state.account.accountID,
					activity: .route
				),
				help: presentation.isCurrent
					? "This account is used by Decodex routing and new Codex processes. Restart ChatGPT to load it there."
					: "Route Decodex now. Restart ChatGPT afterward to load this account; Decodex does not restart it."
			) {
				Task {
					await store.routeAccount(state.account.accountID)
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
	@Binding var isPresentingDetails: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var isLogoutArmed = false

	var body: some View {
		HStack(spacing: PanelSpacing.micro) {
			PanelIconButtonView(
				symbol: "chart.bar.xaxis",
				tint: PanelPalette.actionBlue(colorScheme),
				isActive: isPresentingDetails,
				isDisabled: false,
				isSubtle: true,
				size: 24,
				action: {
					isPresentingDetails.toggle()
				},
				help: "Show account details"
			)
			.popover(isPresented: $isPresentingDetails, arrowEdge: .trailing) {
				AccountProfileDetailView(state: state)
			}

			Menu {
				Button(state.account.enabled ? "Disable account" : "Enable account") {
					Task {
						await store.setAccount(
							state.account.accountID,
							enabled: state.account.enabled == false
						)
					}
				}
				.disabled(lifecycleActionIsDisabled)

				Divider()

				Button("Log out…", role: .destructive) {
					isLogoutArmed = true
				}
				.disabled(lifecycleActionIsDisabled)
			} label: {
				Image(systemName: "ellipsis")
					.font(PanelFont.iconButton)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.frame(width: 24, height: 24)
					.contentShape(Rectangle())
			}
			.menuStyle(.borderlessButton)
			.menuIndicator(.hidden)
			.fixedSize()
			.help("More account actions")
			.accessibilityLabel("More account actions")
			.popover(isPresented: $isLogoutArmed, arrowEdge: .trailing) {
				logoutConfirmation
			}
		}
		.fixedSize(horizontal: true, vertical: false)
		.onChange(of: state.account.accountRevision) {
			isLogoutArmed = false
		}
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

	var body: some View {
		Button {
			guard isDisabled == false else {
				return
			}
			action()
		} label: {
			ZStack {
				HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.compact) {
					Image(systemName: symbol)
						.contentTransition(.symbolEffect(.replace))

					Text(title)
						.contentTransition(.opacity)
				}
					.opacity(isBusy ? 0 : 1)
					.scaleEffect(isBusy ? 0.96 : 1)

				if isBusy {
					ProgressView()
						.controlSize(.mini)
						.transition(
							.opacity.combined(
								with: .scale(scale: 0.88)
							)
						)
						.accessibilityHidden(true)
				}
			}
			.font(PanelFont.compactAction)
			.lineLimit(1)
			.foregroundStyle(
				isActive
					? PanelPalette.routeAccent(colorScheme)
					: PanelPalette.primaryText(colorScheme).opacity(0.88)
			)
			.padding(.horizontal, PanelSpacing.micro)
			.frame(minHeight: 20)
			.contentShape(Rectangle())
		}
		.buttonStyle(PanelPressButtonStyle(pressedScale: 0.97))
		.disabled(usesDisabledEnvironment)
		.allowsHitTesting(isDisabled == false)
		.opacity(isVisuallyDisabled && isActive == false ? 0.44 : 1)
		.animation(controlStateAnimation, value: isActive)
		.animation(controlStateAnimation, value: isBusy)
		.help(help)
		.accessibilityLabel(title)
		.accessibilityHint(help)
		.accessibilityValue(
			isBusy ? "In progress" : (isActive ? "Current" : "")
		)
	}

	private var controlStateAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.controlState
	}
}
