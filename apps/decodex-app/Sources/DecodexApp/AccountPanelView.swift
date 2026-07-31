import AppKit
import SwiftUI

struct AccountPanelView: View {
	let store: ResetCardStore
	private let layoutVisibleFrameOverride: CGRect?
	private let loadsExternalState: Bool
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
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
		ZStack {
			panelContent
				.disabled(store.accountReauthentication != nil)
				.allowsHitTesting(store.accountReauthentication == nil)
				.accessibilityHidden(store.accountReauthentication != nil)

			if store.accountReauthentication != nil {
				reauthenticationOverlay
					.transition(
						.opacity.combined(
							with: .scale(scale: 0.98, anchor: .center)
						)
					)
					.zIndex(1)
			}
		}
		.frame(width: AccountPanelLayout.panelWidth)
		.padding(PanelSpacing.related)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
		.animation(panelLayoutAnimation, value: store.accounts.map(\.id))
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
		.animation(panelLayoutAnimation, value: store.accountReauthentication != nil)
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

	private var panelContent: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.section) {
			headerOverview

			if hasTransientStatus {
				transientStatus
					.transition(.panelSection)
			}

			accountContent
		}
		.animation(panelLayoutAnimation, value: hasTransientStatus)
	}

	private var headerOverview: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.related) {
			header

			if let profileAggregate {
				AccountProfileOverviewView(
					aggregate: profileAggregate
				)
					.transition(.panelSection)
			}
		}
		.padding(.horizontal, PanelSpacing.cardHorizontal)
		.padding(.vertical, PanelSpacing.cardVertical)
		.panelCardSurface(cornerRadius: 18)
		.animation(panelLayoutAnimation, value: profileAggregate != nil)
	}

	private var reauthenticationOverlay: some View {
		ZStack {
			Color.clear
				.contentShape(Rectangle())
				.accessibilityHidden(true)

			AccountReauthenticationView(store: store)
				.panelModalSurface(cornerRadius: 16)
				.accessibilityAddTraits(.isModal)
		}
	}

	private var header: some View {
		HStack(alignment: .center, spacing: PanelSpacing.related) {
			Image(nsImage: AppAssets.statusBarIcon)
				.resizable()
				.renderingMode(.template)
				.scaledToFit()
				.foregroundStyle(PanelPalette.actionBlue(colorScheme))
				.frame(width: 17, height: 17)
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
					accountPrivacy =
						accountPrivacy == AccountPrivacy.hidden
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
				help: fastMode.isEnabled ? "Turn Fast mode off" : "Turn Fast mode on"
			)

			PanelIconButtonView(
				symbol: "plus",
				tint: PanelPalette.actionBlue(colorScheme),
				isActive: false,
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
						? "Show email addresses"
						: "Hide email addresses"
				) {
					accountPrivacy =
						accountPrivacy == AccountPrivacy.hidden
						? AccountPrivacy.visible
						: AccountPrivacy.hidden
				}

				Button(fastMode.isEnabled ? "Turn Fast mode off" : "Turn Fast mode on") {
					Task {
						await fastMode.toggle()
					}
				}
				.disabled(fastMode.isLoading)

				Button("Refresh all") {
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
					Button("Use balanced routing") {
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
		VStack(alignment: .leading, spacing: PanelSpacing.related) {
			if hasBoundedTransientStatus {
				ScrollView(.vertical, showsIndicators: false) {
					VStack(alignment: .leading, spacing: PanelSpacing.related) {
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

	@ViewBuilder
	private var accountContent: some View {
		if store.accounts.isEmpty {
			emptyOrLoadingState
		} else {
			ScrollView(.vertical, showsIndicators: false) {
				VStack(alignment: .leading, spacing: PanelSpacing.section) {
					ForEach(store.accounts) { state in
						ResetCardAccountRow(
							state: state,
							store: store,
							showsEmail: accountPrivacy == AccountPrivacy.visible,
							detailedAccountID: $detailedAccountID
						)
						.panelCardSurface(cornerRadius: 16)
						.transition(.panelSection)
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
			estimated: AccountPanelLayout.estimatedAccountListContentHeight(
				accountCount: store.accounts.count
			)
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

	private var panelLayoutAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.panelLayout
	}

	private var emptyOrLoadingState: some View {
		HStack(alignment: .center, spacing: PanelSpacing.section) {
			if store.isInitialLoading {
				ProgressView()
					.controlSize(.small)
			} else {
				Image(systemName: store.hasLoaded ? "person.2.slash" : "bolt.horizontal.circle")
					.font(PanelFont.emptyIcon)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			}

			VStack(alignment: .leading, spacing: PanelSpacing.micro) {
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
		.padding(.horizontal, PanelSpacing.cardHorizontal)
		.padding(.vertical, PanelSpacing.cardVertical)
		.panelCardSurface(cornerRadius: 16)
	}
}
