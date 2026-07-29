import AppKit
import SwiftUI

struct AccountPanelView: View {
	let store: ResetCardStore
	private let layoutVisibleFrameOverride: CGRect?
	private let loadsExternalState: Bool
	@Environment(\.colorScheme) private var colorScheme
	@State private var panelScreenVisibleFrame: CGRect?
	@State private var measuredAccountListContentHeight: CGFloat = 0
	@State private var accountScrollOffset: CGFloat = 0
	@State private var isPresentingEnrollment = false
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
		GlassEffectContainer(spacing: 6) {
			VStack(alignment: .leading, spacing: 7) {
				header

				if hasTransientStatus {
					transientStatus
						.transition(.panelSection)
				}

				if let profileAggregate {
					AccountProfileOverviewView(
						aggregate: profileAggregate,
						totalAccountCount: store.accounts.count,
						currentProfileCount: profiledAccountStates.filter {
							$0.isProfileDegraded == false
						}.count,
						degradedProfileCount: profiledAccountStates.filter(\.isProfileDegraded).count
					)
					.transition(.panelSection)
				}

				accountContent
			}
			.frame(width: AccountPanelLayout.panelWidth)
			.padding(9)
			.modernGlassSurface(cornerRadius: 18, depth: .panel)
			.controlSize(.small)
			.symbolRenderingMode(.hierarchical)
			.animation(PanelMotion.panelLayout, value: store.accounts.map(\.id))
			.sizesPanelWindowToContent { visibleFrame in
				if panelScreenVisibleFrame != visibleFrame {
					panelScreenVisibleFrame = visibleFrame
				}
			}
		}
		// Re-key the singleton panel, rather than every repeated glass row, when
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
	}

	private var header: some View {
		HStack(alignment: .center, spacing: 5) {
			Image(nsImage: AppAssets.statusBarIcon)
				.resizable()
				.renderingMode(.template)
				.scaledToFit()
				.foregroundStyle(PanelPalette.actionBlue(colorScheme))
				.frame(width: 20, height: 20)
				.frame(width: 28, height: 28)
				.accessibilityHidden(true)

			VStack(alignment: .leading, spacing: 2) {
				Text("Decodex")
					.font(PanelFont.headerTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
				Text(headerSubtitle)
					.font(PanelFont.headerSubtitle)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.minimumScaleFactor(0.82)
			}
			.layoutPriority(1)

			Spacer(minLength: 4)

			PanelIconButtonView(
				symbol: accountPrivacy == AccountPrivacy.hidden ? "eye.slash" : "eye",
				tint: PanelPalette.secondaryText(colorScheme),
				isActive: accountPrivacy == AccountPrivacy.visible,
				isSubtle: true,
				size: 25,
				action: {
					withAnimation(PanelMotion.state) {
						accountPrivacy = accountPrivacy == AccountPrivacy.hidden
							? AccountPrivacy.visible
							: AccountPrivacy.hidden
					}
				},
				help: accountPrivacy == AccountPrivacy.hidden
					? "Show account emails"
					: "Hide account emails"
			)

			PanelIconButtonView(
				symbol: fastMode.isEnabled ? "bolt.fill" : "bolt",
				tint: PanelPalette.fastModeAccent(colorScheme),
				isActive: fastMode.isEnabled,
				isDisabled: fastMode.isLoading,
				isSubtle: fastMode.isEnabled == false,
				size: 25,
				action: {
					Task {
						await fastMode.toggle()
					}
				},
				help: fastMode.errorMessage
					?? (fastMode.isEnabled ? "Turn Fast mode off" : "Turn Fast mode on")
			)

			PanelIconButtonView(
				symbol: "plus",
				tint: PanelPalette.actionBlue(colorScheme),
				isActive: false,
				isDisabled: store.isAccountControlInProgress || store.isRefreshing,
				isPrimary: true,
				size: 25,
				action: {
					isPresentingEnrollment = true
				},
				help: "Add Codex login"
			)

			Menu {
				Button("Refresh All") {
					Task {
						await store.refresh()
					}
				}
				.disabled(store.isRefreshing || store.submittingKey != nil)

				Divider()

				Button("Quit Decodex") {
					NSApplication.shared.terminate(nil)
				}
			} label: {
				Image(systemName: "ellipsis")
					.font(PanelFont.iconButton)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.frame(width: 21, height: 25)
					.contentShape(Rectangle())
			}
			.menuStyle(.borderlessButton)
			.menuIndicator(.hidden)
			.fixedSize()
			.help("More actions")
		}
		.padding(.horizontal, 2)
	}

	private var headerSubtitle: String {
		if store.isInitialLoading, store.accounts.isEmpty {
			return "Loading accounts"
		}
		let count = store.accounts.count
		return "\(count) account\(count == 1 ? "" : "s") · \(routingSubtitle)"
	}

	private var hasTransientStatus: Bool {
		fastMode.errorMessage != nil
			|| store.message != nil
			|| store.pendingAttempts.isEmpty == false
	}

	private var transientStatus: some View {
		ScrollView(.vertical) {
			VStack(alignment: .leading, spacing: 7) {
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

				if let message = store.message {
					ResetCardMessageView(message: message) {
						store.dismissMessage()
					}
				}

				if store.pendingAttempts.isEmpty == false {
					ResetCardPendingAttemptsView(store: store)
				}
			}
		}
		.frame(height: AccountPanelLayout.statusViewportHeight)
		.accessibilityLabel("Decodex status and pending actions")
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
			return "routing unavailable"
		}
		switch routing.mode {
		case .balanced:
			return "balanced"
		case .fixed(let accountID):
			let label = store.accounts.first {
				$0.account.accountID == accountID
			}?.account.displayLabel
			return label.map { "fixed · \($0)" } ?? "fixed"
		}
	}

	@ViewBuilder
	private var accountContent: some View {
		if store.accounts.isEmpty {
			emptyOrLoadingState
		} else {
			ScrollView(.vertical, showsIndicators: false) {
				VStack(alignment: .leading, spacing: 0) {
					ForEach(Array(store.accounts.enumerated()), id: \.element.id) { index, state in
						ResetCardAccountRow(
							state: state,
							store: store,
							showsEmail: accountPrivacy == AccountPrivacy.visible
						)

						if index < store.accounts.count - 1 {
							Rectangle()
								.fill(PanelPalette.separator(colorScheme))
								.frame(height: 0.5)
								.padding(.horizontal, 7)
								.allowsHitTesting(false)
						}
					}
				}
				.background(accountScrollProbe)
				.background(accountRowsHeightProbe)
			}
			.coordinateSpace(name: AccountPanelLayout.accountListScrollSpace)
			.frame(
				height: accountListViewportHeight
			)
			.overlay(alignment: .trailing) {
				AccountListScrollIndicatorView(
					contentHeight: accountListContentHeight,
					viewportHeight: accountListViewportHeight,
					scrollOffset: accountScrollOffset
				)
				.padding(.trailing, 1)
			}
			.onPreferenceChange(AccountScrollOffsetPreferenceKey.self) { minY in
				let maximumOffset = max(0, accountListContentHeight - accountListViewportHeight)
				accountScrollOffset = min(max(0, -minY), maximumOffset)
			}
			.onPreferenceChange(AccountRowsHeightPreferenceKey.self) { height in
				let measuredHeight = ceil(height)
				if abs(measuredAccountListContentHeight - measuredHeight) > 0.5 {
					measuredAccountListContentHeight = measuredHeight
				}
			}
			.onChange(of: accountListNeedsScrolling) { _, needsScrolling in
				if needsScrolling == false {
					accountScrollOffset = 0
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
				? AccountPanelLayout.statusViewportHeight
				: 0
		)
	}

	private var accountListNeedsScrolling: Bool {
		accountListContentHeight > accountListViewportHeight + 1
	}

	private var accountScrollProbe: some View {
		GeometryReader { proxy in
			Color.clear.preference(
				key: AccountScrollOffsetPreferenceKey.self,
				value: proxy.frame(in: .named(AccountPanelLayout.accountListScrollSpace)).minY
			)
		}
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
						? "Import an account with the Decodex CLI, then refresh."
						: "The account service has not returned a complete list."
				)
				.font(PanelFont.emptyBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(9)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
	}
}
