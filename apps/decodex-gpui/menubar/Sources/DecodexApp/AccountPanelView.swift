import AppKit
import SwiftUI

private struct AccountReorderInteraction {
	let token = UUID()
	let accountID: String
	let baseOrder: [String]
	var visualOrder: [String]
	let frames: [String: CGRect]
	var draggedOffsetY: CGFloat
	var isSettling = false
}

struct AccountPanelView: View {
	let store: ResetCardStore
	private let layoutVisibleFrameOverride: CGRect?
	private let loadsExternalState: Bool
	private let onContentSizeChange: (CGSize) -> Void
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme
	@State private var panelScreenVisibleFrame: CGRect?
	@State private var measuredAccountListContentHeight: CGFloat = 0
	@State private var accountCardFrames = [String: CGRect]()
	@State private var accountReorderInteraction: AccountReorderInteraction?
	@State private var hoveredAccountID: String?
	@State private var detailedAccountID: String?
	@State private var fastMode: FastModeStore
	@AppStorage("decodex.operator.accountPrivacy") private var accountPrivacy = AccountPrivacy.hidden
	@AppStorage(PanelCardMaterial.storageKey) private var panelCardMaterialRawValue = PanelCardMaterial.thin.rawValue

	init(
		store: ResetCardStore,
		fastModeStore: FastModeStore = FastModeStore(),
		layoutVisibleFrameOverride: CGRect? = nil,
		loadsExternalState: Bool = true,
		onContentSizeChange: @escaping (CGSize) -> Void = { _ in }
	) {
		self.store = store
		self.layoutVisibleFrameOverride = layoutVisibleFrameOverride
		self.loadsExternalState = loadsExternalState
		self.onContentSizeChange = onContentSizeChange
		_fastMode = State(initialValue: fastModeStore)
	}

	var body: some View {
		// Keep the popover itself transparent and let each section own its
		// floating surface. Grouping the cards in GlassEffectContainer makes
		// Liquid Glass merge them into one enclosing panel.
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
		.environment(\.panelCardMaterial, panelCardMaterial)
		.frame(width: AccountPanelLayout.panelWidth)
		.padding(PanelSpacing.related)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
		.animation(panelLayoutAnimation, value: store.accounts.map(\.id))
		.reportsPanelContentMetrics(
			onVisibleFrameChange: { visibleFrame in
				if panelScreenVisibleFrame != visibleFrame {
					panelScreenVisibleFrame = visibleFrame
				}
			},
			onContentSizeChange: onContentSizeChange
		)
		// Re-key the singleton panel, rather than every repeated card, when
		// system appearance changes.
		.id(colorScheme == .dark ? "account-panel-dark" : "account-panel-light")
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

	private var panelCardMaterial: PanelCardMaterial {
		PanelCardMaterial(rawValue: panelCardMaterialRawValue) ?? .thin
	}

	private var panelCardMaterialSelection: Binding<PanelCardMaterial> {
		Binding(
			get: { panelCardMaterial },
			set: { panelCardMaterialRawValue = $0.rawValue }
		)
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
				isDisabled: store.canBeginEnrollment == false,
				isSubtle: true,
				isPrimary: true,
				size: 24,
				action: {
					store.beginAccountEnrollment()
				},
				help: "Add account"
			)

			Menu {
				Button("Refresh all") {
					store.requestRefresh()
				}
				.disabled(
					store.isAccountControlInProgress
						|| store.submittingKey != nil
				)

				Picker("Material", selection: panelCardMaterialSelection) {
					ForEach(PanelCardMaterial.allCases) { material in
						Text(material.title)
							.tag(material)
					}
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

						if store.pendingAttempts.isEmpty == false,
							store.message?.tone != .error
						{
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
		.accessibilityLabel("Decodex status and pending requests")
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
					ForEach(presentedAccountStates) { state in
						ResetCardAccountRow(
							state: state,
							store: store,
							showsEmail: accountPrivacy == AccountPrivacy.visible,
							detailedAccountID: $detailedAccountID,
							isAccountCardHovered: hoveredAccountID == state.id,
							isReorderGestureEnabled: canDragAccount(state.id),
							onReorderDragChanged: { translationY in
								updateAccountReorder(
									accountID: state.id,
									translationY: translationY
								)
							},
							onReorderDragEnded: {
								finishAccountReorder(accountID: state.id)
							}
						)
						.panelCardSurface(cornerRadius: 16)
						.background {
							GeometryReader { proxy in
								Color.clear.preference(
									key: AccountCardFramesPreferenceKey.self,
									value: [
										state.id: proxy.frame(
											in: .named(
												AccountCardReorderLayout.coordinateSpaceName
											)
										)
									]
								)
							}
						}
						.offset(y: accountReorderOffset(for: state.id))
						.zIndex(isDraggedAccount(state.id) ? 1 : 0)
						.animation(
							accountReorderAnimation(for: state.id),
							value: accountReorderOffset(for: state.id)
						)
						.transition(.panelSection)
					}
				}
				.coordinateSpace(name: AccountCardReorderLayout.coordinateSpaceName)
				.overlay {
					AccountCardHoverTrackingView(
						cardFrames: accountCardFrames,
						onHoveredAccountChanged: updateHoveredAccount
					)
					.accessibilityHidden(true)
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
			.onPreferenceChange(AccountCardFramesPreferenceKey.self) { frames in
				updateAccountCardFrames(frames)
			}
			.accessibilityLabel("Decodex accounts")
		}
	}

	private var presentedAccountStates: [ResetCardAccountState] {
		guard let interaction = accountReorderInteraction else {
			return store.accounts
		}
		let stateByID = Dictionary(
			uniqueKeysWithValues: store.accounts.map { ($0.id, $0) }
		)
		let states = interaction.baseOrder.compactMap { stateByID[$0] }
		return states.count == store.accounts.count ? states : store.accounts
	}

	private func updateHoveredAccount(_ accountID: String?) {
		if hoveredAccountID != accountID {
			hoveredAccountID = accountID
		}
	}

	private func updateAccountCardFrames(_ frames: [String: CGRect]) {
		guard accountReorderInteraction == nil else {
			return
		}
		let accountIDs = Set(store.accounts.map(\.id))
		let currentFrames = frames.filter { accountIDs.contains($0.key) }
		if accountCardFrames != currentFrames {
			accountCardFrames = currentFrames
		}
	}

	private func canDragAccount(_ accountID: String) -> Bool {
		guard store.canReorderAccounts else {
			return false
		}
		guard let interaction = accountReorderInteraction else {
			return true
		}
		return interaction.accountID == accountID
			&& interaction.isSettling == false
	}

	private func updateAccountReorder(
		accountID: String,
		translationY: CGFloat
	) {
		if accountReorderInteraction == nil {
			let baseOrder = store.accounts.map(\.id)
			guard store.canReorderAccounts,
				baseOrder.contains(accountID),
				baseOrder.allSatisfy({ accountCardFrames[$0] != nil })
			else {
				return
			}
			accountReorderInteraction = AccountReorderInteraction(
				accountID: accountID,
				baseOrder: baseOrder,
				visualOrder: baseOrder,
				frames: accountCardFrames,
				draggedOffsetY: 0
			)
		}

		guard var interaction = accountReorderInteraction,
			interaction.accountID == accountID,
			interaction.isSettling == false
		else {
			return
		}
		let constrainedTranslation = AccountCardReorderLayout.constrainedTranslationY(
			for: accountID,
			baseOrder: interaction.baseOrder,
			frames: interaction.frames,
			proposed: translationY
		)
		interaction.draggedOffsetY = constrainedTranslation
		interaction.visualOrder = AccountCardReorderLayout.reorderedAccountIDs(
			dragging: accountID,
			baseOrder: interaction.baseOrder,
			frames: interaction.frames,
			translationY: constrainedTranslation
		)
		accountReorderInteraction = interaction
	}

	private func finishAccountReorder(accountID: String) {
		guard var interaction = accountReorderInteraction,
			interaction.accountID == accountID,
			interaction.isSettling == false
		else {
			return
		}
		interaction.isSettling = true
		interaction.draggedOffsetY = AccountCardReorderLayout.verticalOffset(
			for: accountID,
			baseOrder: interaction.baseOrder,
			visualOrder: interaction.visualOrder,
			frames: interaction.frames,
			spacing: PanelSpacing.section
		)
		accountReorderInteraction = interaction

		let token = interaction.token
		let finalOrder = interaction.visualOrder
		let targetAccountID = accountIDAfter(
			accountID,
			in: finalOrder
		)
		Task {
			if reduceMotion == false {
				try? await Task.sleep(for: .milliseconds(240))
			}
			guard Task.isCancelled == false,
				accountReorderInteraction?.token == token
			else {
				return
			}
			if finalOrder != interaction.baseOrder {
				await store.moveAccounts(
					[accountID],
					before: targetAccountID
				)
			}
			guard accountReorderInteraction?.token == token else {
				return
			}
			let authoritativeOrder = store.accounts.map(\.id)
			let authoritativeFrames =
				AccountCardReorderLayout.rebasedFrames(
					from: interaction.baseOrder,
					to: authoritativeOrder,
					frames: interaction.frames,
					spacing: PanelSpacing.section
				) ?? [:]
			if authoritativeOrder == finalOrder {
				var handoffTransaction = Transaction(animation: nil)
				handoffTransaction.disablesAnimations = true
				withTransaction(handoffTransaction) {
					accountCardFrames = authoritativeFrames
					accountReorderInteraction = nil
				}
			} else {
				accountCardFrames = authoritativeFrames
				accountReorderInteraction = nil
			}
		}
	}

	private func accountIDAfter(
		_ accountID: String,
		in order: [String]
	) -> String? {
		guard let index = order.firstIndex(of: accountID),
			order.indices.contains(index + 1)
		else {
			return nil
		}
		return order[index + 1]
	}

	private func accountReorderOffset(for accountID: String) -> CGFloat {
		guard let interaction = accountReorderInteraction else {
			return 0
		}
		if interaction.accountID == accountID {
			return interaction.draggedOffsetY
		}
		return AccountCardReorderLayout.verticalOffset(
			for: accountID,
			baseOrder: interaction.baseOrder,
			visualOrder: interaction.visualOrder,
			frames: interaction.frames,
			spacing: PanelSpacing.section
		)
	}

	private func isDraggedAccount(_ accountID: String) -> Bool {
		accountReorderInteraction?.accountID == accountID
	}

	private func accountReorderAnimation(for accountID: String) -> Animation? {
		guard reduceMotion == false else {
			return nil
		}
		if let interaction = accountReorderInteraction,
			interaction.accountID == accountID,
			interaction.isSettling == false
		{
			return nil
		}
		return PanelMotion.accountReorder
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
