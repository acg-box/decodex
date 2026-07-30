import SwiftUI

struct AccountPrimaryActionsView: View {
	let state: ResetCardAccountState
	let store: ResetCardStore

	var body: some View {
		HStack(spacing: 4) {
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
		HStack(spacing: 2) {
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
				Button(state.account.enabled ? "Disable Account" : "Enable Account") {
					Task {
						await store.setAccount(
							state.account.accountID,
							enabled: state.account.enabled == false
						)
					}
				}
				.disabled(lifecycleActionIsDisabled)

				Divider()

				Button("Log Out…", role: .destructive) {
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
		VStack(alignment: .leading, spacing: 9) {
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

				Button("Log Out", role: .destructive) {
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
		.padding(12)
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
			title: "Refresh Login",
			symbol: "person.crop.circle.badge.plus",
			isActive: false,
			isDisabled: isDisabled,
			isBusy: store.isControllingAccount(
				state.account.accountID,
				activity: .loginRefresh
			),
			help: "Refresh the saved login for this account from the matching shared Codex login."
		) {
			Task {
				await store.refreshCredentials(for: state.account.accountID)
			}
		}
	}

	private var isDisabled: Bool {
		state.account.credentialBinding == nil
			|| store.isAwaitingFreshAccountSkeleton(state.account.accountID)
			|| store.canPerformDirectAccountControl == false
			|| store.isControllingAccount(state.account.accountID)
			|| store.isEnrollingAccount
			|| store.isRoutingAccountControl
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
			.padding(.horizontal, 2)
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
		VStack(alignment: .leading, spacing: 10) {
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
				.disabled(store.isEnrollingAccount)
			}
		}
		.frame(width: 260)
		.padding(14)
	}
}
