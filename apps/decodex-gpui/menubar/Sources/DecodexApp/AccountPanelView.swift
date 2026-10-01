import AppKit
import SwiftUI

struct AccountReorderInteraction {
	let token = UUID()
	let accountID: String
	let baseOrder: [String]
	var visualOrder: [String]
	let frames: [String: CGRect]
	var draggedOffsetY: CGFloat
	var isSettling = false

	func isCurrent(for accountIDs: [String]) -> Bool {
		accountIDs == baseOrder || (isSettling && accountIDs == visualOrder)
	}

	func presentedAccounts(_ accounts: [ResetCardAccountState]) -> [ResetCardAccountState] {
		guard isCurrent(for: accounts.map(\.id)) else { return accounts }
		let stateByID = Dictionary(
			uniqueKeysWithValues: accounts.map { ($0.id, $0) }
		)
		let states = baseOrder.compactMap { stateByID[$0] }
		return states.count == accounts.count ? states : accounts
	}
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
	@State private var detailedAccountIDs: Set<String> = []
	@State private var fastMode: FastModeStore
	@AppStorage("decodex.operator.accountPrivacy") private var accountPrivacy = AccountPrivacy.hidden
	@AppStorage(PanelCardMaterial.storageKey) private var panelCardMaterialRawValue = PanelCardMaterial.thin.rawValue

	init(
		store: ResetCardStore,
		fastModeStore: FastModeStore,
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
		.onChange(of: store.accounts.map(\.id)) { _, accountIDs in
			if let interaction = accountReorderInteraction,
				!interaction.isCurrent(for: accountIDs) {
				accountReorderInteraction = nil
			}
		}
		.onDisappear { accountReorderInteraction = nil }
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


			accountContent
		}
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
			if let feedback = globalFeedback {
				InlineAccountFeedback(text: feedback)
			}

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

	private var globalFeedback: String? {
		if let error = fastMode.errorMessage { return error }
		if let message = store.message, message.tone != .success,
			message.accountID == nil || !store.accounts.contains(where: { $0.id == message.accountID }) {
			return message.text
		}
		if let attempt = store.pendingAttempts.first(where: { attempt in
			!store.accounts.contains(where: { $0.id == attempt.target.accountID })
		}) { return store.pendingStatus(for: attempt).text }
		return nil
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
				AccountRows(
					rows: presentedAccountStates.map { state in
						AccountRowContent(
							id: state.id,
							content: AnyView(
								ResetCardAccountRow(
									state: state, store: store,
									showsEmail: accountPrivacy == AccountPrivacy.visible,
									detailedAccountIDs: $detailedAccountIDs,
									detailsExpanded: detailedAccountIDs.contains(state.id),
									isAccountCardHovered: hoveredAccountID == state.id,
									isReorderGestureEnabled: canDragAccount(state.id),
									onReorderDragChanged: { updateAccountReorder(accountID: state.id, translationY: $0) },
									onReorderDragEnded: { finishAccountReorder(accountID: state.id) }
								)
								.panelCardSurface(cornerRadius: 16)
								.scaleEffect(isDraggedAccount(state.id) && !reduceMotion ? 1.012 : 1)
								.shadow(color: .black.opacity(isDraggedAccount(state.id) ? 0.16 : 0), radius: 8, y: 3)
								.animation(reduceMotion ? nil : PanelMotion.controlState, value: isDraggedAccount(state.id))
								.environment(\.colorScheme, colorScheme)
								.environment(\.panelCardMaterial, panelCardMaterial)
								.controlSize(.small)
								.symbolRenderingMode(.hierarchical)
							),
							offset: accountReorderOffset(for: state.id),
							isDragging: isDraggedAccount(state.id),
							renderState: AccountRowRenderState(
								account: state, store: ObjectIdentifier(store),
								expanded: detailedAccountIDs.contains(state.id),
								showsEmail: accountPrivacy == AccountPrivacy.visible,
								hovered: hoveredAccountID == state.id,
								canDrag: canDragAccount(state.id), dragging: isDraggedAccount(state.id),
								colorScheme: colorScheme, material: panelCardMaterial
							)
						)
					},
					reduceMotion: reduceMotion,
					onHeightChange: { measuredAccountListContentHeight = $0 },
					onFramesChange: updateAccountCardFrames
				)
				.frame(height: measuredAccountListContentHeight > 0 ? measuredAccountListContentHeight : nil, alignment: .top)
				.overlay {
					AccountCardHoverTrackingView(cardFrames: accountCardFrames, onHoveredAccountChanged: updateHoveredAccount)
						.accessibilityHidden(true)
				}
			}
			.frame(
				height: accountListViewportHeight
			)
			.accessibilityLabel("Decodex accounts")
		}
	}

	private var activeReorderInteraction: AccountReorderInteraction? {
		guard let interaction = accountReorderInteraction,
			interaction.isCurrent(for: store.accounts.map(\.id)) else { return nil }
		return interaction
	}

	private var presentedAccountStates: [ResetCardAccountState] {
		guard let interaction = activeReorderInteraction else {
			return store.accounts
		}
		return interaction.presentedAccounts(store.accounts)
	}

	private func updateHoveredAccount(_ accountID: String?) {
		if hoveredAccountID != accountID {
			hoveredAccountID = accountID
		}
	}

	private func updateAccountCardFrames(_ frames: [String: CGRect]) {
		guard activeReorderInteraction == nil else {
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
		guard let interaction = activeReorderInteraction else {
			return true
		}
		return interaction.accountID == accountID
			&& interaction.isSettling == false
	}

	private func updateAccountReorder(
		accountID: String,
		translationY: CGFloat
	) {
		if activeReorderInteraction == nil {
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

		guard var interaction = activeReorderInteraction,
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
		guard var interaction = activeReorderInteraction,
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
				activeReorderInteraction?.token == token
			else {
				return
			}
			if finalOrder != interaction.baseOrder {
				await store.moveAccounts(
					[accountID],
					before: targetAccountID
				)
			}
			guard activeReorderInteraction?.token == token else {
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
		guard let interaction = activeReorderInteraction else {
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
		activeReorderInteraction?.accountID == accountID
	}

	private var accountListViewportHeight: CGFloat {
		AccountPanelLayout.accountListHeight(
			accountCount: store.accounts.count,
			measuredContentHeight: measuredAccountListContentHeight,
			windowVisibleFrame: layoutVisibleFrameOverride ?? panelScreenVisibleFrame
		)
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
							? "Add a Codex login to get started."
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
