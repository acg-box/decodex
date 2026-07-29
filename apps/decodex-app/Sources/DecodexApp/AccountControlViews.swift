import SwiftUI

struct AccountRowActionsView: View {
	let state: ResetCardAccountState
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var isLogoutArmed = false
	@State private var isRenaming = false
	@State private var renameLabel = ""

	var body: some View {
		Group {
			if isLogoutArmed {
				logoutConfirmation
			} else {
				defaultActions
			}
		}
		.animation(PanelMotion.state, value: isLogoutArmed)
		.onChange(of: state.account.accountRevision) {
			isLogoutArmed = false
		}
	}

	private var defaultActions: some View {
		HStack(spacing: 2) {
			if showsLoginRecovery {
				PanelIconButtonView(
					symbol: "person.crop.circle.badge.plus",
					tint: PanelPalette.warning(colorScheme),
					isActive: false,
					isDisabled: controlsAreDisabled,
					isPrimary: true,
					size: 20,
					action: {
						Task {
							await store.refreshCredentials(for: state.account.accountID)
						}
					},
					help: "Sign in again for this account"
				)
			}

			Menu {
				Button(
					isFixed ? "Use Balanced Routing" : "Route Only to This Account"
				) {
					Task {
						if isFixed {
							await store.selectBalancedAccounts()
						} else {
							await store.selectFixedAccount(state.account.accountID)
						}
					}
				}
				.disabled(controlsAreDisabled || canSelect == false)

				Divider()

				Button("Rename…") {
					renameLabel = state.account.displayLabel
					isRenaming = true
				}

				Button(state.account.enabled ? "Disable" : "Enable") {
					Task {
						await store.setAccount(
							state.account.accountID,
							enabled: state.account.enabled == false
						)
					}
				}
				.disabled(controlsAreDisabled)

				Button("Refresh Login") {
					Task {
						await store.refreshCredentials(for: state.account.accountID)
					}
				}
				.disabled(
					controlsAreDisabled || state.account.credentialBinding == nil
				)

				Divider()

				Button("Log Out", role: .destructive) {
					isLogoutArmed = true
				}
			} label: {
				Image(systemName: "ellipsis")
					.font(PanelFont.iconButton)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.frame(width: 20, height: 20)
					.contentShape(Rectangle())
			}
			.menuStyle(.borderlessButton)
			.menuIndicator(.hidden)
			.fixedSize()
			.disabled(controlsAreDisabled)
			.help("Account actions")
			.popover(isPresented: $isRenaming, arrowEdge: .top) {
				renamePopover
			}
		}
	}

	private var logoutConfirmation: some View {
		HStack(spacing: 4) {
			Button {
				isLogoutArmed = false
			} label: {
				Image(systemName: "xmark")
					.frame(width: 16, height: 18)
			}
			.buttonStyle(.plain)
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.disabled(controlsAreDisabled)
			.help("Keep this account")

			Button("Log Out", role: .destructive) {
				Task {
					await store.logoutAccount(state.account.accountID)
					isLogoutArmed = false
				}
			}
			.buttonStyle(.borderless)
			.controlSize(.mini)
			.foregroundStyle(PanelPalette.destructive(colorScheme))
			.disabled(controlsAreDisabled)
		}
		.fixedSize()
	}

	private var renamePopover: some View {
		VStack(alignment: .leading, spacing: 9) {
			Text("Rename account")
				.font(.headline)

			TextField("Display name", text: $renameLabel)
				.textFieldStyle(.roundedBorder)
				.frame(width: 220)

			HStack {
				Button("Cancel") {
					isRenaming = false
				}
				.keyboardShortcut(.cancelAction)

				Spacer()

				Button("Save") {
					let label = renameLabel.trimmingCharacters(in: .whitespacesAndNewlines)
					Task {
						await store.renameAccount(
							state.account.accountID,
							displayLabel: label
						)
						isRenaming = false
					}
				}
				.keyboardShortcut(.defaultAction)
				.disabled(
					renameLabel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
						|| controlsAreDisabled
				)
			}
		}
		.padding(12)
	}

	private var controlsAreDisabled: Bool {
		store.isRefreshing
			|| store.isAccountControlInProgress
			|| store.submittingKey != nil
	}

	private var canSelect: Bool {
		state.account.enabled
			&& state.account.lifecycleReadiness == .ready
			&& state.account.unsettledOperation == nil
	}

	private var isFixed: Bool {
		guard case .fixed(let accountID) = store.routing?.mode else {
			return false
		}
		return accountID == state.account.accountID
	}

	private var showsLoginRecovery: Bool {
		state.account.credentialBinding != nil
			&& (
				state.account.observedState == .authFailed
					|| state.profileUnavailable?.error == .unauthorized
			)
	}
}

struct AccountEnrollmentView: View {
	let store: ResetCardStore
	let dismiss: () -> Void
	@State private var displayLabel = ""

	var body: some View {
		VStack(alignment: .leading, spacing: 10) {
			Text("Add Codex login")
				.font(.headline)

			Text("Import the account currently signed in to Codex.")
				.font(.callout)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)

			TextField("Display name", text: $displayLabel)
				.textFieldStyle(.roundedBorder)
				.frame(width: 250)

			HStack {
				Button("Cancel", action: dismiss)
					.keyboardShortcut(.cancelAction)

				Spacer()

				Button("Add") {
					let label = displayLabel.trimmingCharacters(in: .whitespacesAndNewlines)
					Task {
						await store.enrollFromSharedCodex(displayLabel: label)
						if store.message?.tone != .error {
							dismiss()
						}
					}
				}
				.keyboardShortcut(.defaultAction)
				.disabled(
					displayLabel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
						|| store.isEnrollingAccount
				)
			}
		}
		.padding(14)
	}
}
