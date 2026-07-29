import SwiftUI

struct AccountRowActionsView: View {
	let state: ResetCardAccountState
	let store: ResetCardStore
	@Binding var isPresentingDetails: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var isLogoutArmed = false

	var body: some View {
		HStack(spacing: 4) {
			CompactAccountActionButton(
				title: "Use in Codex",
				symbol: isCodexProjection
					? "checkmark.circle.fill"
					: "arrow.right.circle",
				isActive: isCodexProjection,
				isDisabled: codexActionIsDisabled,
				isBusy: store.isControllingAccount(
					state.account.accountID,
					activity: .codexProjection
				),
				help: isCodexProjection
					? "This account is projected to the shared Codex login."
					: "Use this account for new Codex processes. This does not change Decodex routing."
			) {
				Task {
					await store.useAccountInCodex(state.account.accountID)
				}
			}

			CompactAccountActionButton(
				title: isFixed ? "Routed" : "Route",
				symbol: isFixed ? "point.3.connected.trianglepath.dotted" : "arrow.triangle.branch",
				isActive: isFixed,
				isDisabled: routingActionIsDisabled,
				isBusy: store.isRoutingAccountControl,
				help: isFixed
					? "Decodex is routed only to this account."
					: "Route Decodex only to this account. This does not change the shared Codex login."
			) {
				Task {
					await store.selectFixedAccount(state.account.accountID)
				}
			}

			CompactAccountActionButton(
				title: "Details",
				symbol: "chart.bar.xaxis",
				isActive: isPresentingDetails,
				isDisabled: false,
				isBusy: false,
				help: "Show saved activity, plan, and freshness details for this account."
			) {
				isPresentingDetails.toggle()
			}
			.popover(isPresented: $isPresentingDetails, arrowEdge: .trailing) {
				AccountProfileDetailView(state: state)
			}

			Spacer(minLength: 0)

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
		.onChange(of: state.account.accountRevision) {
			isLogoutArmed = false
		}
	}

	private var logoutConfirmation: some View {
		VStack(alignment: .leading, spacing: 9) {
			Text("Log out this account?")
				.font(.headline)

			Text("The account and its saved credential binding will be removed from Decodex.")
				.font(.callout)
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

	private var isCodexProjection: Bool {
		store.isCodexProjection(state.account.accountID)
	}

	private var codexActionIsDisabled: Bool {
		isCodexProjection
			|| canSelect == false
			|| store.canPerformDirectAccountControl == false
			|| store.isControllingAccount(state.account.accountID)
			|| store.isEnrollingAccount
			|| store.isRoutingAccountControl
			|| store.submittingKey != nil
	}

	private var routingActionIsDisabled: Bool {
		isFixed
			|| canSelect == false
			|| store.canPerformDirectAccountControl == false
			|| store.isRoutingAccountControl
			|| store.isAccountControlInProgress
			|| store.submittingKey != nil
	}

	private var lifecycleActionIsDisabled: Bool {
		store.isRefreshing
			|| store.isRefreshingAccountSkeleton
			|| store.isAccountControlInProgress
			|| store.isAwaitingFreshAccountSkeleton(state.account.accountID)
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
					: PanelPalette.secondaryText(colorScheme)
			)
			.padding(.horizontal, 5)
			.frame(minHeight: 24)
			.background(
				isActive
					? PanelPalette.routeAccent(colorScheme).opacity(
						colorScheme == .dark ? 0.16 : 0.1
					)
					: PanelPalette.progressTrack(colorScheme).opacity(0.54)
			)
			.clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
			.contentShape(Rectangle())
		}
		.buttonStyle(.plain)
		.disabled(isDisabled)
		.opacity(isDisabled ? 0.48 : 1)
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
				.font(.headline)

			Text(
				"Import the account currently signed in to Codex. "
					+ "Decodex assigns a stable account alias."
			)
			.font(.callout)
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
