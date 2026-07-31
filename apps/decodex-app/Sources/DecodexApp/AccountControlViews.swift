import SwiftUI

struct AccountPrimaryActionsView: View {
	let state: ResetCardAccountState
	let store: ResetCardStore

	var body: some View {
		HStack(spacing: PanelSpacing.compact) {
			CompactAccountActionButton(
				title: isRouteCurrent ? "Routed" : "Route",
				symbol: isRouteCurrent
					? "point.3.connected.trianglepath.dotted"
					: "arrow.triangle.branch",
				isActive: isRouteCurrent,
				isDisabled: routeActionIsDisabled,
				isBusy: store.isControllingAccount(
					state.account.accountID,
					activity: .route
				),
				help: isRouteCurrent
					? "This account is used by Decodex routing and new Codex processes."
					: "Route Decodex and use this account for new Codex processes."
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

	private var routeActionIsDisabled: Bool {
		isRouteCurrent
			|| canSelect == false
			|| store.canPerformDirectAccountControl == false
			|| store.isAccountControlInProgress
			|| store.submittingKey != nil
	}

	private var canSelect: Bool {
		state.account.enabled
			&& state.account.lifecycleReadiness == .ready
			&& state.account.unsettledOperation == nil
			&& store.isAwaitingFreshAccountSkeleton(state.account.accountID) == false
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
		store.isRefreshing
			|| store.isRefreshingAccountSkeleton
			|| store.isAccountControlInProgress
			|| store.isAwaitingFreshAccountSkeleton(state.account.accountID)
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
			isBusy: store.isControllingAccount(
				state.account.accountID,
				activity: .loginRefresh
			),
			help: "Sign in to this account with the official Codex device login."
		) {
			store.beginAccountReauthentication(for: state.account.accountID)
		}
	}

	private var isDisabled: Bool {
		state.account.credentialBinding == nil
			|| store.isAwaitingFreshAccountSkeleton(state.account.accountID)
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
	let isBusy: Bool
	let help: String
	let action: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Button(action: action) {
			ZStack {
				Label(title, systemImage: symbol)
					.opacity(isBusy ? 0 : 1)

				if isBusy {
					ProgressView()
						.controlSize(.mini)
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
		.buttonStyle(.plain)
		.disabled(isDisabled)
		.opacity(isDisabled && isActive == false ? 0.44 : 1)
		.help(help)
		.accessibilityLabel(title)
		.accessibilityHint(help)
		.accessibilityValue(
			isBusy ? "In progress" : (isActive ? "Current" : "")
		)
	}
}

struct AccountEnrollmentView: View {
	let store: ResetCardStore
	let dismiss: () -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.section) {
			Text("Add Codex login")
				.font(PanelFont.transientTitle)

			Text(
				"Import the account currently signed in to Codex. "
					+ "Decodex assigns a stable account alias."
			)
			.font(PanelFont.transientBody)
			.foregroundStyle(.secondary)
			.fixedSize(horizontal: false, vertical: true)

			HStack {
				Button("Cancel", action: dismiss)
					.keyboardShortcut(.cancelAction)

				Spacer()

				Button("Add") {
					Task {
						await store.enrollFromSharedCodex()
						if store.message?.tone != .error {
							dismiss()
						}
					}
				}
				.keyboardShortcut(.defaultAction)
				.disabled(store.canBeginEnrollment == false)
			}
		}
		.frame(width: 260)
		.padding(PanelSpacing.popoverInset)
	}
}
