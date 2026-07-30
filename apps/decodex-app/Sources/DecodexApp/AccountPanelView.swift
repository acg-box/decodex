import AppKit
import SwiftUI

struct AccountPanelView: View {
	let store: ResetCardStore
	private let layoutVisibleFrameOverride: CGRect?
	private let loadsExternalState: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var panelScreenVisibleFrame: CGRect?
	@State private var measuredAccountListContentHeight: CGFloat = 0
	@State private var isPresentingEnrollment = false
	@State private var detailedAccountID: String?
	@State private var fastMode: FastModeStore
	@AppStorage("decodex.operator.accountPrivacy") private var accountPrivacy = AccountPrivacy.hidden

	init(
		store: ResetCardStore,
		fastModeStore: FastModeStore = FastModeStore(),
		layoutVisibleFrameOverride: CGRect? = nil,
		loadsExternalState: Bool = true
	) {
		self.store = store
		self.layoutVisibleFrameOverride = layoutVisibleFrameOverride
		self.loadsExternalState = loadsExternalState
		_fastMode = State(initialValue: fastModeStore)
	}

	var body: some View {
		VStack(alignment: .leading, spacing: 7) {
			header
				.padding(.horizontal, 10)
				.padding(.vertical, 8)
				.panelCardSurface(cornerRadius: 18)

			if hasTransientStatus {
				transientStatus
					.transition(.panelSection)
			}

			if let profileAggregate {
				AccountProfileOverviewView(
					aggregate: profileAggregate
				)
				.panelCardSurface(cornerRadius: 16)
				.transition(.panelSection)
			}

			accountContent
		}
		.frame(width: AccountPanelLayout.panelWidth)
		.padding(6)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
		.animation(PanelMotion.panelLayout, value: store.accounts.map(\.id))
		.sizesPanelWindowToContent { visibleFrame in
			if panelScreenVisibleFrame != visibleFrame {
				panelScreenVisibleFrame = visibleFrame
			}
		}
		// Re-key the singleton panel, rather than every repeated card, when
		// system appearance changes.
		.id(colorScheme == .dark ? "account-panel-dark" : "account-panel-light")
		.sheet(isPresented: $isPresentingEnrollment) {
			AccountEnrollmentView(store: store) {
				isPresentingEnrollment = false
			}
		}
		.task {
			guard loadsExternalState else {
				return
			}
			if fastMode.hasLoaded == false {
				await fastMode.load()
			}
		}
		.task(id: accountPrivacy) {
			guard loadsExternalState else {
				return
			}
			await store.setProfileEmailVisibility(accountPrivacy == AccountPrivacy.visible)
		}
		.task(id: store.message?.text) {
			guard store.message?.tone == .success else {
				return
			}
			let displayedText = store.message?.text
			try? await Task.sleep(for: .seconds(2))
			guard Task.isCancelled == false,
				store.message?.tone == .success,
				store.message?.text == displayedText
			else {
				return
			}
			store.dismissMessage()
		}
	}

	private var header: some View {
		VStack(alignment: .leading, spacing: 4) {
			HStack(alignment: .center, spacing: 6) {
				Image(nsImage: AppAssets.statusBarIcon)
					.resizable()
					.renderingMode(.template)
					.scaledToFit()
					.foregroundStyle(PanelPalette.actionBlue(colorScheme))
					.frame(width: 19, height: 19)
					.frame(width: 26, height: 26)
					.accessibilityHidden(true)

				Text("Decodex")
					.font(PanelFont.headerTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))

				Spacer(minLength: 4)

				PanelIconButtonView(
					symbol: accountPrivacy == AccountPrivacy.visible ? "eye" : "eye.slash",
					tint: PanelPalette.actionBlue(colorScheme),
					isActive: accountPrivacy == AccountPrivacy.visible,
					isSubtle: true,
					size: 24,
					action: {
						accountPrivacy = accountPrivacy == AccountPrivacy.hidden
							? AccountPrivacy.visible
							: AccountPrivacy.hidden
					},
					help: accountPrivacy == AccountPrivacy.hidden
						? "Show email addresses"
						: "Hide email addresses"
				)

				PanelIconButtonView(
					symbol: fastMode.isEnabled ? "bolt.fill" : "bolt",
					tint: PanelPalette.fastModeAccent(colorScheme),
					isActive: fastMode.isEnabled,
					isDisabled: fastMode.isLoading,
					isSubtle: true,
					size: 24,
					action: {
						Task {
							await fastMode.toggle()
						}
					},
					help: fastMode.isEnabled ? "Turn Fast Mode off" : "Turn Fast Mode on"
				)

				PanelIconButtonView(
					symbol: "plus",
					tint: PanelPalette.actionBlue(colorScheme),
					isActive: false,
					isDisabled: store.isAccountControlInProgress
						|| store.isRefreshing
						|| store.isRefreshingAccountSkeleton,
					isSubtle: true,
					isPrimary: true,
					size: 24,
					action: {
						isPresentingEnrollment = true
					},
					help: "Add Codex login"
				)

				Menu {
					Button(
						accountPrivacy == AccountPrivacy.hidden
							? "Show Email Addresses"
							: "Hide Email Addresses"
					) {
						accountPrivacy = accountPrivacy == AccountPrivacy.hidden
							? AccountPrivacy.visible
							: AccountPrivacy.hidden
					}

					Button(fastMode.isEnabled ? "Turn Fast Mode Off" : "Turn Fast Mode On") {
						Task {
							await fastMode.toggle()
						}
					}
					.disabled(fastMode.isLoading)

					Button("Refresh All") {
						Task {
							await store.refresh()
						}
					}
					.disabled(
						store.isRefreshing
							|| store.isRefreshingAccountSkeleton
							|| store.isAccountControlInProgress
							|| store.submittingKey != nil
					)

					if case .fixed = store.routing?.mode {
						Button("Use Balanced Routing") {
							Task {
								await store.selectBalancedAccounts()
							}
						}
						.disabled(
							store.canPerformDirectAccountControl == false
								|| store.isRoutingAccountControl
								|| store.submittingKey != nil
						)
					}

					Divider()

					Button("Quit Decodex") {
						NSApplication.shared.terminate(nil)
					}
				} label: {
					Image(systemName: "ellipsis")
						.font(PanelFont.iconButton)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.frame(width: 26, height: 26)
						.contentShape(Rectangle())
				}
				.menuStyle(.borderlessButton)
				.menuIndicator(.hidden)
				.fixedSize()
				.help("Decodex menu")
				.accessibilityLabel("Decodex menu")
			}

			VStack(alignment: .leading, spacing: 1) {
				headerState(label: "Routing", value: routingSubtitle)
				.accessibilityLabel("Decodex routing, \(routingSubtitle)")
				headerState(label: "Codex", value: codexProjectionSubtitle)
				.accessibilityLabel("Shared Codex login, \(codexProjectionSubtitle)")
			}
			.padding(.leading, 32)
		}
		.padding(.horizontal, 2)
	}

	private var hasTransientStatus: Bool {
		hasBoundedTransientStatus
			|| displayedIntrinsicMessage != nil
	}

	private var hasBoundedTransientStatus: Bool {
		fastMode.errorMessage != nil
			|| store.message?.tone == .error
			|| store.pendingAttempts.isEmpty == false
	}

	private var transientStatus: some View {
		VStack(alignment: .leading, spacing: 5) {
			if hasBoundedTransientStatus {
				ScrollView(.vertical, showsIndicators: false) {
					VStack(alignment: .leading, spacing: 5) {
						if let errorMessage = fastMode.errorMessage {
							ResetCardMessageView(
								message: ResetCardStoreMessage(
									tone: .error,
									text: errorMessage
								)
							) {
								fastMode.dismissError()
							}
						}

						if let message = store.message, message.tone == .error {
							ResetCardMessageView(message: message) {
								store.dismissMessage()
							}
						}

						if store.pendingAttempts.isEmpty == false {
							ResetCardPendingAttemptsView(store: store)
						}
					}
				}
				.frame(maxHeight: AccountPanelLayout.statusMaximumHeight)
			}

			if let message = displayedIntrinsicMessage {
				ResetCardMessageView(message: message) {
					store.dismissMessage()
				}
			}
		}
		.accessibilityLabel("Decodex status and pending actions")
	}

	private var displayedIntrinsicMessage: ResetCardStoreMessage? {
		guard let message = store.message, message.tone != .error else {
			return nil
		}
		if message.tone == .success,
			message.text == "Fixed account selected."
				|| message.text == "Balanced account selection enabled."
		{
			return nil
		}
		return message
	}

	private var profileAggregate: AccountProfileAggregate? {
		AccountProfileAggregate.make(
			profiledAccountStates.compactMap { $0.profile?.snapshot }
		)
	}

	private var profiledAccountStates: [ResetCardAccountState] {
		store.accounts.filter {
			$0.profile?.snapshot.hasContent == true
		}
	}

	private var routingSubtitle: String {
		guard let routing = store.routing else {
			return store.isInitialLoading ? "Loading" : "Unavailable"
		}
		switch routing.mode {
		case .balanced:
			return "Balanced"
		case .fixed(let accountID):
			let state = store.accounts.first {
				$0.account.accountID == accountID
			}
			return state.map(accountIdentity(for:)) ?? "Fixed account"
		}
	}

	private var codexProjectionSubtitle: String {
		let state = store.accounts.first(where: {
			store.isCodexProjection($0.account.accountID)
		})
		return CodexProjectionPresentation(
			projection: store.codexAuthProjection,
			currentIdentity: state.map(accountIdentity(for:)),
			isInitialLoading: store.isInitialLoading
		).text
	}

	private func accountIdentity(for state: ResetCardAccountState) -> String {
		AccountIdentityPresentation(
			alias: state.account.alias,
			email: state.profile?.email ?? state.profileUnavailable?.claims.email,
			revealsEmail: accountPrivacy == AccountPrivacy.visible
		).text
	}

	private func headerState(label: String, value: String) -> some View {
		HStack(alignment: .firstTextBaseline, spacing: 4) {
			Text("\(label):")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.frame(width: 43, alignment: .leading)

			Text(value)
				.font(PanelFont.headerSubtitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.9))
				.lineLimit(1)
				.truncationMode(.middle)
		}
	}

	@ViewBuilder
	private var accountContent: some View {
		if store.accounts.isEmpty {
			emptyOrLoadingState
		} else {
			ScrollView(.vertical, showsIndicators: false) {
				VStack(alignment: .leading, spacing: 7) {
					ForEach(store.accounts) { state in
						ResetCardAccountRow(
							state: state,
							store: store,
							showsEmail: accountPrivacy == AccountPrivacy.visible,
							detailedAccountID: $detailedAccountID
						)
						.panelCardSurface(cornerRadius: 16)
					}
				}
				.padding(1)
				.background(accountRowsHeightProbe)
			}
			.frame(
				height: accountListViewportHeight
			)
			.onPreferenceChange(AccountRowsHeightPreferenceKey.self) { height in
				let measuredHeight = ceil(height)
				if abs(measuredAccountListContentHeight - measuredHeight) > 0.5 {
					measuredAccountListContentHeight = measuredHeight
				}
			}
			.accessibilityLabel("Decodex accounts")
		}
	}

	private var accountListContentHeight: CGFloat {
		AccountPanelLayout.resolvedAccountListContentHeight(
			measured: measuredAccountListContentHeight,
			estimated: CGFloat(max(1, store.accounts.count))
				* AccountPanelLayout.estimatedAccountRowHeight
		)
	}

	private var accountListViewportHeight: CGFloat {
		AccountPanelLayout.accountListHeight(
			accountCount: store.accounts.count,
			measuredContentHeight: measuredAccountListContentHeight,
			windowVisibleFrame: layoutVisibleFrameOverride ?? panelScreenVisibleFrame,
				additionalChromeHeight: hasTransientStatus
					? AccountPanelLayout.statusMaximumHeight
					: 0
		)
	}

	private var accountRowsHeightProbe: some View {
		GeometryReader { proxy in
			Color.clear.preference(
				key: AccountRowsHeightPreferenceKey.self,
				value: proxy.size.height
			)
		}
	}

	private var emptyOrLoadingState: some View {
		HStack(alignment: .center, spacing: 8) {
			if store.isInitialLoading {
				ProgressView()
					.controlSize(.small)
			} else {
				Image(systemName: store.hasLoaded ? "person.2.slash" : "bolt.horizontal.circle")
					.font(PanelFont.emptyIcon)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			}

			VStack(alignment: .leading, spacing: 2) {
				Text(store.isInitialLoading ? "Loading accounts" : "No accounts")
					.font(PanelFont.emptyTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
					Text(
						store.hasLoaded
							? "Add a Codex login, then refresh."
							: "The account service has not returned a complete list."
				)
				.font(PanelFont.emptyBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(9)
		.panelCardSurface(cornerRadius: 16)
	}
}
