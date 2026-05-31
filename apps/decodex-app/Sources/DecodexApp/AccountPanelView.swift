import AppKit
import Foundation
import SwiftUI

enum PanelPalette {
	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.95, green: 0.96, blue: 0.98).opacity(0.97)
			: Color(red: 0.12, green: 0.14, blue: 0.18).opacity(0.94)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.73, green: 0.76, blue: 0.82).opacity(0.84)
			: Color(red: 0.34, green: 0.38, blue: 0.45).opacity(0.8)
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.065)
			: Color(red: 0.32, green: 0.38, blue: 0.46).opacity(0.14)
	}

	static func actionBlue(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.86, green: 0.89, blue: 0.94)
			: Color(red: 0.18, green: 0.29, blue: 0.4)
	}

	static func codexAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.82, green: 0.87, blue: 0.94)
			: Color(red: 0.2, green: 0.36, blue: 0.52)
	}

	static func routeAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.68, green: 0.8, blue: 0.96)
			: Color(red: 0.13, green: 0.32, blue: 0.56)
	}

	static func landingAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.74, green: 0.82, blue: 0.9)
			: Color(red: 0.19, green: 0.34, blue: 0.46)
	}

	static func capacityAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.72, green: 0.8, blue: 0.88)
			: Color(red: 0.18, green: 0.34, blue: 0.48)
	}

	static func warning(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.95, green: 0.68, blue: 0.38)
			: Color(red: 0.62, green: 0.4, blue: 0.12)
	}

	static func usageCyan(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.35, green: 0.78, blue: 0.86)
			: Color(red: 0.1, green: 0.53, blue: 0.62)
	}

	static func fastModeAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.98, green: 0.84, blue: 0.48)
			: Color(red: 0.42, green: 0.31, blue: 0.09)
	}

	static func destructive(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.98, green: 0.4, blue: 0.45)
			: Color(red: 0.68, green: 0.1, blue: 0.16)
	}

	static func progressTrack(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.09)
			: Color(red: 0.15, green: 0.23, blue: 0.3).opacity(0.1)
	}

	static func progressEdge(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.12)
			: Color.white.opacity(0.22)
	}

	static func glassStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.1)
			: Color.white.opacity(0.5)
	}

	static func glassInnerShadow(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.black.opacity(0.18)
			: Color.black.opacity(0.055)
	}
}

enum PanelMotion {
	static let hover = Animation.interactiveSpring(response: 0.22, dampingFraction: 0.86, blendDuration: 0.04)
	static let press = Animation.interactiveSpring(response: 0.16, dampingFraction: 0.78, blendDuration: 0.02)
	static let state = Animation.interactiveSpring(response: 0.24, dampingFraction: 0.88, blendDuration: 0.05)
}

private extension View {
	func panelInteractiveSurface(
		isPressed: Bool = false,
		isDisabled: Bool = false,
		hoverLift: CGFloat = 0.7,
		hoverScale: CGFloat = 1.006,
		pressedScale: CGFloat = 0.985,
		hoverShadowRadius: CGFloat = 3
	) -> some View {
		modifier(
			PanelInteractiveSurfaceModifier(
				isPressed: isPressed,
				isDisabled: isDisabled,
				hoverLift: hoverLift,
				hoverScale: hoverScale,
				pressedScale: pressedScale,
				hoverShadowRadius: hoverShadowRadius
			)
		)
	}
}

private struct PanelInteractiveButtonStyle: ButtonStyle {
	let isDisabled: Bool
	let hoverLift: CGFloat
	let hoverScale: CGFloat
	let pressedScale: CGFloat
	let hoverShadowRadius: CGFloat

	init(
		isDisabled: Bool = false,
		hoverLift: CGFloat = 0.7,
		hoverScale: CGFloat = 1.006,
		pressedScale: CGFloat = 0.985,
		hoverShadowRadius: CGFloat = 3
	) {
		self.isDisabled = isDisabled
		self.hoverLift = hoverLift
		self.hoverScale = hoverScale
		self.pressedScale = pressedScale
		self.hoverShadowRadius = hoverShadowRadius
	}

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.panelInteractiveSurface(
				isPressed: configuration.isPressed,
				isDisabled: isDisabled,
				hoverLift: hoverLift,
				hoverScale: hoverScale,
				pressedScale: pressedScale,
				hoverShadowRadius: hoverShadowRadius
			)
	}
}

private struct PanelInteractiveSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false
	let isPressed: Bool
	let isDisabled: Bool
	let hoverLift: CGFloat
	let hoverScale: CGFloat
	let pressedScale: CGFloat
	let hoverShadowRadius: CGFloat

	func body(content: Content) -> some View {
		let responds = isDisabled == false
		let hoverActive = responds && isHovered && isPressed == false
		let pressActive = responds && isPressed

		content
			.scaleEffect(pressActive ? pressedScale : (hoverActive ? hoverScale : 1))
			.offset(y: hoverActive ? -hoverLift : 0)
			.brightness(hoverActive ? hoverBrightness : (pressActive ? pressBrightness : 0))
			.shadow(
				color: hoverShadowColor.opacity(hoverActive ? 1 : 0),
				radius: hoverActive ? hoverShadowRadius : 0,
				x: 0,
				y: hoverActive ? 1.8 : 0
			)
			.onHover { hovering in
				guard responds else {
					return
				}

				withAnimation(PanelMotion.hover) {
					isHovered = hovering
				}
			}
			.animation(PanelMotion.press, value: isPressed)
			.animation(PanelMotion.hover, value: isHovered)
			.animation(PanelMotion.state, value: isDisabled)
	}

	private var hoverBrightness: Double {
		colorScheme == .dark ? 0.022 : 0.016
	}

	private var pressBrightness: Double {
		colorScheme == .dark ? 0.006 : -0.004
	}

	private var hoverShadowColor: Color {
		colorScheme == .dark
			? Color.black.opacity(0.18)
			: Color.black.opacity(0.09)
	}
}

struct AccountPanelView: View {
	@ObservedObject var store: AccountStore
	@ObservedObject var loginWindowState: LoginWindowState
	@Environment(\.colorScheme) private var colorScheme
	@State private var accountScrollOffset: CGFloat = 0
	@State private var pendingLogout: CodexAccount?
	@State private var armedLogoutAccountID: String?
	@State private var logoutArmToken = UUID()
	@AppStorage("decodex.operator.accountPrivacy") private var accountPrivacy = AccountPrivacy.hiddenValue

	var body: some View {
		Group {
			if #available(macOS 26.0, *) {
				GlassEffectContainer(spacing: 6) {
					panelContent
				}
			} else {
				panelContent
			}
		}
		.confirmationDialog(
			"Remove account?",
			isPresented: Binding(
				get: { pendingLogout != nil },
				set: { visible in
					if visible == false {
						pendingLogout = nil
						disarmLogout()
					}
				}
			),
			titleVisibility: .visible
		) {
			if let account = pendingLogout {
				Button("Remove \(displayName(for: account))", role: .destructive) {
					disarmLogout()
					Task {
						await store.logout(account)
					}
				}
			}
		} message: {
			if let account = pendingLogout {
				Text("This removes \(displayName(for: account)) from the Decodex account pool on this Mac.")
			}
		}
		.background {
			LoginPanelPresenter(store: store, state: loginWindowState)
				.frame(width: 0, height: 0)
		}
	}

	private var panelContent: some View {
		VStack(alignment: .leading, spacing: 6) {
			header
			accountSummary

			if let usageEstimate = store.accountList?.usageEstimate {
				AccountPoolUsageEstimateView(estimate: usageEstimate, accounts: store.accounts)
			}

			if let snapshot = store.operatorSnapshot, snapshot.shouldDisplayInPanel {
				OperatorStatusStripView(
					snapshot: snapshot,
					updatedAt: store.operatorSnapshotUpdatedAt
				)
			}

			if let notice = store.notice {
				NoticeView(text: notice)
			}

			if let usageProbeError = store.accountList?.usageProbeError {
				NoticeView(text: "Usage probe: \(usageProbeError)")
			}

			if store.isInitialLoading {
				loadingState
			} else if store.accounts.isEmpty {
				emptyState
			} else {
				accountList
			}
		}
		.frame(width: 322)
		.padding(9)
		.modernGlassSurface(
			cornerRadius: 18,
			depth: .panel
		)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
	}

	private var header: some View {
		HStack(alignment: .center, spacing: 8) {
			Image(nsImage: AppAssets.statusBarIcon)
				.resizable()
				.renderingMode(.template)
				.scaledToFit()
				.foregroundStyle(PanelPalette.actionBlue(colorScheme))
				.frame(width: 20, height: 20)
				.frame(width: 28, height: 28)

			VStack(alignment: .leading, spacing: 2) {
				Text("Decodex")
					.font(PanelFont.headerTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
				Text(headerSubtitle)
					.font(PanelFont.headerSubtitle)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.minimumScaleFactor(0.9)
			}
			.layoutPriority(1)

			Spacer(minLength: 4)

			HStack(spacing: 5) {
				PanelIconButtonView(
					symbol: emailsHidden ? "eye.slash" : "eye",
					tint: PanelPalette.secondaryText(colorScheme),
					isActive: false,
					action: {
						accountPrivacy = emailsHidden ? AccountPrivacy.visibleValue : AccountPrivacy.hiddenValue
					},
					help: emailsHidden ? "Show account emails" : "Hide account emails"
				)

				PanelIconButtonView(
					symbol: store.fastModeEnabled ? "bolt.fill" : "bolt",
					tint: PanelPalette.fastModeAccent(colorScheme),
					isActive: store.fastModeEnabled,
					isDisabled: store.isSettingFastMode,
					size: 25,
					action: {
						Task {
							await store.setFastMode(store.fastModeEnabled == false)
						}
					},
					help: store.fastModeEnabled ? "Turn fast mode off" : "Turn fast mode on"
				)

				PanelIconButtonView(
					symbol: "safari",
					tint: PanelPalette.actionBlue(colorScheme),
					isActive: false,
					action: {
						Task {
							await store.openWebUI()
						}
					},
					help: "Open Decodex WebUI"
				)

				if hasFixedSelection {
					PanelIconButtonView(
						symbol: "shuffle",
						tint: PanelPalette.actionBlue(colorScheme),
						isActive: false,
						action: {
							Task {
								await store.clearSelection()
							}
						},
						help: "Restore balanced run routing"
					)
					.transition(.opacity.combined(with: .scale(scale: 0.96)))
				}

				PanelIconButtonView(
					symbol: "plus",
					tint: PanelPalette.actionBlue(colorScheme),
					isActive: false,
					isPrimary: true,
					size: 25,
					action: {
						presentLogin(.newAccount)
					},
					help: "Add login"
				)
			}
		}
		.animation(PanelMotion.state, value: hasFixedSelection)
	}

	private var accountSummary: some View {
		HStack(spacing: 7) {
			SummaryTileView(
				title: "Codex",
				value: codexAuthLabel,
				symbol: "person.crop.circle",
				tint: PanelPalette.codexAccent(colorScheme)
			)

			Rectangle()
				.fill(PanelPalette.separator(colorScheme))
				.frame(width: 0.5, height: 16)

			SummaryTileView(
				title: "Runs",
				value: decodexModeLabel,
				symbol: "arrow.triangle.branch",
				tint: hasFixedSelection ? PanelPalette.actionBlue(colorScheme) : PanelPalette.secondaryText(colorScheme)
			)
		}
		.padding(.horizontal, 3)
		.padding(.top, 1)
		.padding(.bottom, 4)
		.overlay(alignment: .bottom) {
			Rectangle()
				.fill(PanelPalette.separator(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.9))
				.frame(height: 0.5)
				.allowsHitTesting(false)
		}
	}

	private var emptyState: some View {
		VStack(alignment: .leading, spacing: 6) {
			Image(systemName: "person.crop.circle.badge.plus")
				.font(PanelFont.emptyIcon)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			Text("No accounts in the local pool")
				.font(PanelFont.emptyTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
			Text("Add a ChatGPT login before switching the Codex auth file.")
				.font(PanelFont.emptyBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(8)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
	}

	private var loadingState: some View {
		HStack(spacing: 7) {
			ProgressView()
				.controlSize(.small)
			Text("Loading accounts")
				.font(PanelFont.emptyTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
			Spacer()
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(8)
		.modernGlassSurface(cornerRadius: 9, depth: .row)
	}

	private var accountList: some View {
		Group {
			if accountListNeedsScrolling {
				ScrollView(.vertical, showsIndicators: false) {
					accountRows
						.background(accountScrollProbe)
				}
				.coordinateSpace(name: AccountPanelLayout.accountListScrollSpace)
				.frame(height: accountListViewportHeight)
				.overlay(alignment: .trailing) {
					AccountListScrollIndicatorView(
						contentHeight: accountListContentHeight,
						viewportHeight: accountListViewportHeight,
						scrollOffset: accountScrollOffset
					)
					.padding(.trailing, 1)
				}
				.onPreferenceChange(AccountScrollOffsetPreferenceKey.self) { minY in
					let maxOffset = max(0, accountListContentHeight - accountListViewportHeight)
					accountScrollOffset = min(max(0, -minY), maxOffset)
				}
			} else {
				accountRows
			}
		}
	}

	private var accountRows: some View {
		VStack(spacing: 0) {
			ForEach(Array(store.accounts.enumerated()), id: \.element.id) { index, account in
				let runs = operatorRuns(for: account)

				AccountRowView(
					account: account,
					runs: runs,
					displayName: displayName(for: account),
					showsDivider: index < store.accounts.count - 1,
					isLogoutArmed: armedLogoutAccountID == account.id,
					useInCodex: {
						Task {
							await store.useInCodex(account)
						}
					},
					routeRunsHere: {
						Task {
							await store.select(account)
						}
					},
					login: {
						presentLogin(.account(displayName(for: account)))
					},
					logout: {
						requestLogout(account)
					}
				)
			}
		}
		.padding(.top, 1)
	}

	private var codexAuthLabel: String {
		guard let auth = store.accountList?.codexAuth else {
			return "No Codex auth"
		}

		if emailsHidden {
			if let account = account(matching: auth.selector) {
				return displayName(for: account)
			}
			let identity = auth.accountFingerprint.isEmpty ? auth.selector : auth.accountFingerprint
			return AccountDisplay.alias(forIdentity: identity)
		}

		return AccountDisplay.compactEmail(auth.displayName)
	}

	private var decodexModeLabel: String {
		guard let control = store.accountList?.control else {
			return "Not loaded"
		}

		if let selector = control.accountSelector, selector.isEmpty == false {
			if emailsHidden {
				if let account = account(matching: selector) {
					return "To \(displayName(for: account))"
				}

				return "To \(AccountDisplay.alias(forIdentity: selector))"
			}

			if selector.contains("@") {
				return "To \(AccountDisplay.compactEmail(selector))"
			}

			return "To \(AccountDisplay.compactIdentity(selector))"
		}

		if control.mode == "balanced" {
			return "Balanced"
		}

		return control.mode.replacingOccurrences(of: "_", with: " ").capitalized
	}

	private var hasFixedSelection: Bool {
		guard let selector = store.accountList?.control.accountSelector else {
			return false
		}

		return selector.isEmpty == false
	}

	private var accountListContentHeight: CGFloat {
		store.accounts.reduce(CGFloat(1)) { total, account in
			total + accountRowHeight(for: account)
		}
	}

	private var accountListViewportHeight: CGFloat {
		min(accountListContentHeight, accountListAvailableHeight)
	}

	private var accountListNeedsScrolling: Bool {
		accountListContentHeight > accountListAvailableHeight + 1
	}

	private var accountListAvailableHeight: CGFloat {
		let visibleHeight = AccountPanelLayout.activeScreenVisibleHeight()
		let availableHeight = visibleHeight - accountPanelChromeHeight
		let minimumHeight = min(
			AccountPanelLayout.minimumScrollableListHeight,
			max(140, visibleHeight * 0.42)
		)

		return max(minimumHeight, availableHeight)
	}

	private var accountPanelChromeHeight: CGFloat {
		var height = AccountPanelLayout.screenVerticalMargin
			+ AccountPanelLayout.panelVerticalPadding
			+ AccountPanelLayout.headerHeight
			+ AccountPanelLayout.accountSummaryHeight
			+ AccountPanelLayout.sectionSpacing

		if store.accountList?.usageEstimate != nil {
			height += AccountPanelLayout.sectionSpacing + AccountPanelLayout.poolUsageHeight
		}
		if let snapshot = store.operatorSnapshot, snapshot.shouldDisplayInPanel {
			height += AccountPanelLayout.sectionSpacing
				+ (snapshot.warningSummary == nil
					? AccountPanelLayout.operatorStatusHeight
					: AccountPanelLayout.operatorStatusHeightWithWarning)
		}
		if store.notice != nil {
			height += AccountPanelLayout.sectionSpacing + AccountPanelLayout.noticeHeight
		}
		if store.accountList?.usageProbeError != nil {
			height += AccountPanelLayout.sectionSpacing + AccountPanelLayout.noticeHeight
		}

		return height
	}

	private var accountScrollProbe: some View {
		GeometryReader { proxy in
			Color.clear.preference(
				key: AccountScrollOffsetPreferenceKey.self,
				value: proxy.frame(in: .named(AccountPanelLayout.accountListScrollSpace)).minY
			)
		}
	}

	private var headerSubtitle: String {
		let count = store.accounts.count
		let accountLabel = "\(count) account\(count == 1 ? "" : "s")"
		let routeLabel = hasFixedSelection ? "Routed" : "Balanced"
		return "\(accountLabel) · \(routeLabel)"
	}

	private var emailsHidden: Bool {
		accountPrivacy != AccountPrivacy.visibleValue
	}

	private func displayName(for account: CodexAccount) -> String {
		if emailsHidden {
			return AccountDisplay.aliases(for: store.accounts)[account.id]
				?? account.panelDisplayName(emailsHidden: true)
		}

		return account.panelDisplayName(emailsHidden: false)
	}

	private func operatorRuns(for account: CodexAccount) -> [OperatorRunStatus] {
		store.operatorSnapshot?.activeRuns(for: account) ?? []
	}

	private func accountRowHeight(for account: CodexAccount) -> CGFloat {
		let base: CGFloat
		if account.hasUsageWindowSummary {
			base = account.hasSevenDayUsageEstimate ? 120 : 102
		} else if account.hasSevenDayUsageEstimate {
			base = 66
		} else {
			base = 48
		}
		let runSignal: CGFloat = operatorRuns(for: account).isEmpty ? 0 : 22

		return base + runSignal
	}

	private func account(matching selector: String) -> CodexAccount? {
		store.accounts.first { account in
			account.matchesSelector(selector)
		}
	}

	private func requestLogout(_ account: CodexAccount) {
		if armedLogoutAccountID == account.id {
			pendingLogout = account
			disarmLogout()
			return
		}

		let token = UUID()
		logoutArmToken = token
		withAnimation(PanelMotion.state) {
			armedLogoutAccountID = account.id
		}

		Task { @MainActor in
			try? await Task.sleep(nanoseconds: 2_400_000_000)
			guard logoutArmToken == token, armedLogoutAccountID == account.id else {
				return
			}
			withAnimation(PanelMotion.state) {
				armedLogoutAccountID = nil
			}
		}
	}

	private func disarmLogout() {
		logoutArmToken = UUID()
		withAnimation(PanelMotion.state) {
			armedLogoutAccountID = nil
		}
	}

	private func presentLogin(_ mode: AccountLoginSheetMode) {
		loginWindowState.mode = mode
		store.resetLoginSession()
		NSApp.activate(ignoringOtherApps: true)
		loginWindowState.isPresented = true
	}
}

struct AccountRowView: View {
	let account: CodexAccount
	let runs: [OperatorRunStatus]
	let displayName: String
	let showsDivider: Bool
	let isLogoutArmed: Bool
	let useInCodex: () -> Void
	let routeRunsHere: () -> Void
	let login: () -> Void
	let logout: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			HStack(alignment: .center, spacing: 8) {
				HStack(alignment: .firstTextBaseline, spacing: 5) {
					Text(displayName)
						.font(PanelFont.accountName)
						.foregroundStyle(PanelPalette.primaryText(colorScheme))
						.lineLimit(1)
						.truncationMode(.middle)
						.layoutPriority(1)

					Text("·")
						.font(PanelFont.accountDetail)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.62))
						.fixedSize(horizontal: true, vertical: false)

					Text(account.capacityLabel)
						.font(PanelFont.accountDetail)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.lineLimit(1)
						.fixedSize(horizontal: true, vertical: false)

					if let healthLabel = account.compactHealthLabel {
						Text("·")
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.62))
							.fixedSize(horizontal: true, vertical: false)

						Text(healthLabel)
							.font(PanelFont.accountDetail)
							.foregroundStyle(account.statusDisplayColor(colorScheme: colorScheme))
							.lineLimit(1)
							.fixedSize(horizontal: true, vertical: false)
					}

				}
				.frame(maxWidth: .infinity, alignment: .leading)

				HStack(spacing: 3) {
					if account.needsLogin {
						PanelIconButtonView(
							symbol: "person.crop.circle.badge.plus",
							tint: PanelPalette.warning(colorScheme),
							isActive: false,
							isPrimary: true,
							size: 21,
							action: login,
							help: "Login account"
						)
					} else {
						PanelIconButtonView(
							symbol: account.codexActive ? "person.crop.circle.fill" : "person.crop.circle",
							tint: PanelPalette.codexAccent(colorScheme),
							isActive: account.codexActive,
							isDisabled: account.codexActive || account.canUseInCodex == false,
							isSubtle: true,
							size: 21,
							action: useInCodex,
							help: account.codexActive ? "Current Codex account" : "Use as Codex account"
						)
					}

					PanelIconButtonView(
						symbol: "arrow.triangle.branch",
						tint: account.selected
							? PanelPalette.routeAccent(colorScheme)
							: PanelPalette.actionBlue(colorScheme),
						isActive: account.selected,
						isDisabled: account.canRouteRuns == false && account.selected == false,
						isSubtle: true,
						size: 21,
						action: routeRunsHere,
						help: routeHelp
					)

					PanelIconButtonView(
						symbol: isLogoutArmed ? "trash.fill" : "trash",
						tint: PanelPalette.destructive(colorScheme),
						isActive: isLogoutArmed,
						isDestructive: true,
						isSubtle: isLogoutArmed == false,
						size: 21,
						action: logout,
						help: isLogoutArmed ? "Click again to confirm removal" : "Remove account"
					)
					.modifier(DeleteArmedShakeModifier(isArmed: isLogoutArmed))
				}
			}

			if runs.isEmpty == false {
				AccountRunSummaryView(runs: runs)
			}

			if account.hasUsageSummary {
				AccountUsageSummaryView(account: account)
			}
		}
		.padding(.vertical, 7)
		.padding(.leading, 8)
		.padding(.trailing, 7)
		.overlay(alignment: .bottom) {
			if showsDivider {
				Rectangle()
					.fill(PanelPalette.separator(colorScheme).opacity(colorScheme == .dark ? 0.48 : 0.72))
					.frame(height: 0.5)
					.padding(.leading, 8)
					.padding(.trailing, 7)
					.allowsHitTesting(false)
			}
		}
		.animation(PanelMotion.state, value: account.selected)
		.animation(PanelMotion.state, value: account.codexActive)
		.animation(PanelMotion.state, value: isLogoutArmed)
	}

	private var routeHelp: String {
		if account.selected {
			return "Restore balanced run routing"
		}
		if account.needsLogin {
			return "Login before routing runs"
		}
		if account.disabled {
			return "Disabled account cannot route runs"
		}

		return "Route Decodex runs here"
	}
}

struct AccountRunSummaryView: View {
	let runs: [OperatorRunStatus]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		ViewThatFits(in: .horizontal) {
			runRow(visibleCount: 2, style: .detailed)
			runRow(visibleCount: 1, style: .detailed)
			runRow(visibleCount: 3, style: .compact)
			runRow(visibleCount: 2, style: .compact)
			runRow(visibleCount: 1, style: .compact)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}

	private func runRow(
		visibleCount: Int,
		style: AccountRunChipStyle
	) -> some View {
		let visibleRuns = Array(runs.prefix(visibleCount))
		let hiddenRuns = Array(runs.dropFirst(visibleCount))

		return HStack(spacing: 5) {
			ForEach(visibleRuns) { run in
				AccountRunChipView(
					run: run,
					style: style,
					maxWidth: chipMaxWidth(style: style, visibleCount: visibleRuns.count)
				)
			}

			if hiddenRuns.isEmpty == false {
				AccountRunOverflowView(runs: runs, hiddenRunCount: hiddenRuns.count)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}

	private func chipMaxWidth(
		style: AccountRunChipStyle,
		visibleCount: Int
	) -> CGFloat {
		switch style {
		case .detailed:
			return visibleCount <= 1
				? AccountRunChipLayout.wideDetailedMaxWidth
				: AccountRunChipLayout.detailedMaxWidth
		case .compact:
			return AccountRunChipLayout.compactMaxWidth
		}
	}
}

private enum AccountPanelLayout {
	static let accountListScrollSpace = "account-list-scroll"
	static let screenVerticalMargin: CGFloat = 44
	static let panelVerticalPadding: CGFloat = 18
	static let sectionSpacing: CGFloat = 6
	static let headerHeight: CGFloat = 28
	static let accountSummaryHeight: CGFloat = 31
	static let poolUsageHeight: CGFloat = 58
	static let operatorStatusHeight: CGFloat = 42
	static let operatorStatusHeightWithWarning: CGFloat = 63
	static let noticeHeight: CGFloat = 44
	static let minimumScrollableListHeight: CGFloat = 312

	static func activeScreenVisibleHeight() -> CGFloat {
		let mouseLocation = NSEvent.mouseLocation
		let screen = NSScreen.screens.first { screen in
			screen.frame.contains(mouseLocation)
		} ?? NSScreen.main

		return screen?.visibleFrame.height ?? 760
	}
}

private struct AccountScrollOffsetPreferenceKey: PreferenceKey {
	static let defaultValue: CGFloat = 0

	static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
		value = nextValue()
	}
}

private struct AccountListScrollIndicatorView: View {
	let contentHeight: CGFloat
	let viewportHeight: CGFloat
	let scrollOffset: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		if contentHeight > viewportHeight + 1 {
			ZStack(alignment: .top) {
				Capsule(style: .continuous)
					.fill(PanelPalette.secondaryText(colorScheme).opacity(0.12))
					.frame(width: 3, height: viewportHeight)

				Capsule(style: .continuous)
					.fill(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.42 : 0.34))
					.frame(width: 3.5, height: thumbHeight)
					.offset(y: thumbOffset)
			}
			.frame(width: 8, height: viewportHeight)
			.allowsHitTesting(false)
		}
	}

	private var thumbHeight: CGFloat {
		max(30, viewportHeight * min(1, viewportHeight / max(contentHeight, 1)))
	}

	private var thumbOffset: CGFloat {
		let maxScrollOffset = max(1, contentHeight - viewportHeight)
		let maxThumbOffset = max(0, viewportHeight - thumbHeight)
		let progress = min(1, max(0, scrollOffset / maxScrollOffset))

		return maxThumbOffset * progress
	}
}

private enum AccountRunChipLayout {
	static let compactMaxWidth: CGFloat = 108
	static let detailedMaxWidth: CGFloat = 132
	static let wideDetailedMaxWidth: CGFloat = 218
}

enum AccountRunChipStyle {
	case detailed
	case compact
}

struct AccountRunChipView: View {
	let run: OperatorRunStatus
	let style: AccountRunChipStyle
	let maxWidth: CGFloat
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false
	@State private var showsPopover = false

	var body: some View {
		HStack(spacing: 5) {
			Image(systemName: symbol)
				.font(PanelFont.summaryIcon)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.88 : 0.76))
				.frame(width: 11)

			Text(run.compactTitle)
				.font(PanelFont.metricValue)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.92))
				.lineLimit(1)
				.truncationMode(.middle)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: 21)
		.padding(.horizontal, 8)
		.frame(maxWidth: maxWidth, alignment: .leading)
		.background {
			RoundedRectangle(cornerRadius: 10.5, style: .continuous)
				.fill(isHovered ? tint.opacity(colorScheme == .dark ? 0.09 : 0.07) : Color.clear)
		}
		.modernGlassSurface(cornerRadius: 10.5, depth: .control)
		.contentShape(RoundedRectangle(cornerRadius: 10.5, style: .continuous))
		.onHover { hovering in
			withAnimation(PanelMotion.hover) {
				isHovered = hovering
			}
			showsPopover = hovering
		}
		.popover(isPresented: $showsPopover, arrowEdge: .trailing) {
			OperatorLanePopoverView(run: run)
				.frame(width: 360)
				.padding(8)
		}
	}

	private var symbol: String {
		if run.hasAttentionTone {
			return "exclamationmark.triangle.fill"
		}
		if run.isWaiting {
			return "clock"
		}

		return "play.fill"
	}

	private var tint: Color {
		if run.hasAttentionTone {
			return PanelPalette.warning(colorScheme)
		}
		if run.isWaiting {
			return PanelPalette.secondaryText(colorScheme)
		}

		return PanelPalette.routeAccent(colorScheme)
	}
}

struct AccountRunOverflowView: View {
	let runs: [OperatorRunStatus]
	let hiddenRunCount: Int
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false
	@State private var showsPopover = false

	var body: some View {
		Text("+\(hiddenRunCount)")
			.font(PanelFont.metricLabel)
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.frame(height: 21)
			.padding(.horizontal, 7)
			.background {
				RoundedRectangle(cornerRadius: 10.5, style: .continuous)
					.fill(isHovered ? PanelPalette.routeAccent(colorScheme).opacity(0.08) : Color.clear)
			}
			.modernGlassSurface(cornerRadius: 10.5, depth: .control)
			.fixedSize(horizontal: true, vertical: false)
			.contentShape(RoundedRectangle(cornerRadius: 10.5, style: .continuous))
			.onHover { hovering in
				withAnimation(PanelMotion.hover) {
					isHovered = hovering
				}
				showsPopover = hovering
			}
			.popover(isPresented: $showsPopover, arrowEdge: .trailing) {
				OperatorLaneDetailsListView(
					title: "\(runs.count) running lane\(runs.count == 1 ? "" : "s")",
					runs: runs
				)
				.frame(width: 372)
				.padding(8)
			}
	}
}

struct OperatorLaneDetailsListView: View {
	let title: String
	let runs: [OperatorRunStatus]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			HStack(spacing: 7) {
				Image(systemName: "arrow.triangle.branch")
					.font(PanelFont.summaryIcon)
					.foregroundStyle(PanelPalette.routeAccent(colorScheme).opacity(0.86))
					.frame(width: 12)

				Text(title)
					.font(PanelFont.lanePopoverTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
					.lineLimit(1)
			}

			ScrollView {
				VStack(alignment: .leading, spacing: 8) {
					ForEach(runs) { run in
						OperatorLanePopoverView(run: run)
					}
				}
			}
			.frame(maxHeight: 430)
			.scrollIndicators(.hidden)
		}
		.padding(10)
		.modernGlassSurface(cornerRadius: 12, depth: .section)
		.accessibilityLabel(title)
	}
}

struct AccountPoolUsageEstimateView: View {
	let estimate: AccountUsageEstimate
	let accounts: [CodexAccount]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			HStack(spacing: 0) {
				AccountPoolUsageMetricView(
					title: "Pool used",
					value: formatUsagePercent(estimate.totalUsedOfCapacityPercent),
					tint: poolUsageTint
				)

				usageDivider

				AccountPoolUsageMetricView(
					title: "Day Δ",
					value: dayDeltaText,
					tint: dayDeltaTint
				)

				usageDivider

				AccountPoolUsageMetricView(
					title: "Daily avg",
					value: formatDailyUsageRate(estimate.averageDailyPoolPercent),
					tint: PanelPalette.secondaryText(colorScheme)
				)
			}
			.frame(height: 30)

			if estimate.accountEstimateCount < estimate.accountCount {
				Text("\(estimate.accountEstimateCount)/\(estimate.accountCount) accounts measured")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
					.lineLimit(1)
			}
		}
		.padding(.horizontal, 6)
		.padding(.vertical, 5)
		.frame(maxWidth: .infinity, alignment: .leading)
		.modernGlassSurface(cornerRadius: 10, depth: .section)
		.accessibilityLabel(accessibilityLabel)
	}

	private var usageDivider: some View {
		Rectangle()
			.fill(PanelPalette.separator(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.9))
			.frame(width: 0.5, height: 20)
	}

	private var accessibilityLabel: String {
		"Pool usage over \(estimate.windowDays) days: \(formatUsagePercent(estimate.totalUsedOfCapacityPercent)) used, daily change \(dayDeltaText), average \(formatUsagePercent(estimate.averageDailyPoolPercent)) per day"
	}

	private var dayDeltaText: String {
		guard let delta = dayDeltaPercentagePoints else {
			return "-"
		}

		return formatPercentagePointDelta(delta)
	}

	private var poolUsageTint: Color {
		let used = estimate.totalUsedOfCapacityPercent
		if used >= 90 {
			return PanelPalette.destructive(colorScheme)
		}
		if used >= 75 {
			return PanelPalette.warning(colorScheme)
		}

		return PanelPalette.routeAccent(colorScheme)
	}

	private var dayDeltaTint: Color {
		guard let delta = dayDeltaPercentagePoints else {
			return PanelPalette.secondaryText(colorScheme)
		}
		if delta > 0.05 {
			if estimate.totalUsedOfCapacityPercent >= 90 {
				return PanelPalette.destructive(colorScheme)
			}
			if estimate.totalUsedOfCapacityPercent >= 75 {
				return PanelPalette.warning(colorScheme)
			}

			return PanelPalette.capacityAccent(colorScheme)
		}
		if delta < -0.05 {
			return PanelPalette.secondaryText(colorScheme)
		}

		return PanelPalette.secondaryText(colorScheme)
	}

	private var dayDeltaPercentagePoints: Double? {
		let measuredAccounts = accounts.filter { account in
			account.sevenDayUsedPercent != nil
		}
		guard measuredAccounts.isEmpty == false, estimate.totalCapacityPercent > 0 else {
			return nil
		}

		let latestDate = measuredAccounts
			.flatMap(\.recentUsageRecords)
			.map(\.date)
			.max()
		guard let latestDate else {
			return estimate.averageDailyPoolPercent
		}
		guard let previousDate = previousUsageDate(before: latestDate) else {
			return estimate.averageDailyPoolPercent
		}

		let previousRecords = measuredAccounts.compactMap { account in
			usageRecord(for: account, on: previousDate).map { (account, $0) }
		}
		guard previousRecords.count == measuredAccounts.count else {
			return estimate.averageDailyPoolPercent
		}
		let previousUsedPercent = previousRecords.reduce(0) { total, pair in
			let (account, record) = pair

			return total + record.usedPercent * (record.capacityMultiplier ?? account.capacityWeight)
		}
		let previousPoolPercent =
			(Double(previousUsedPercent) / Double(estimate.totalCapacityPercent)) * 100

		return estimate.totalUsedOfCapacityPercent - previousPoolPercent
	}

	private func usageRecord(
		for account: CodexAccount,
		on date: String
	) -> AccountUsageRecord? {
		account.recentUsageRecords
			.filter { record in record.date == date }
			.max { left, right in
				left.checkedAtUnixEpoch < right.checkedAtUnixEpoch
			}
	}

	private func previousUsageDate(before value: String) -> String? {
		let formatter = DateFormatter()
		formatter.locale = Locale(identifier: "en_US_POSIX")
		formatter.dateFormat = "yyyy-MM-dd"
		let calendar = Calendar(identifier: .gregorian)

		guard let date = formatter.date(from: value),
			let previousDate = calendar.date(byAdding: .day, value: -1, to: date)
		else {
			return nil
		}

		return formatter.string(from: previousDate)
	}
}

struct AccountPoolUsageMetricView: View {
	let title: String
	let value: String
	let tint: Color
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 1) {
			Text(title)
				.font(PanelFont.metricLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)

			Text(value)
				.font(PanelFont.metricValue)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.94 : 0.78))
				.monospacedDigit()
				.lineLimit(1)
				.minimumScaleFactor(0.72)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(.horizontal, 5)
	}
}

struct AccountUsageSummaryView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(spacing: 5) {
			if account.hasSevenDayUsageEstimate {
				AccountSevenDayUsageLineView(account: account)
			}

			if account.hasPrimaryUsageData {
				AccountUsageMeterView(
					label: account.windowLabel(seconds: account.primaryWindowSeconds),
					remainingPercent: account.primaryRemainingPercent,
					resetAtUnixEpoch: account.primaryResetsAtUnixEpoch,
					tone: account.usageTone(remainingPercent: account.primaryRemainingPercent)
				)
			}

			if account.hasSecondaryUsageData {
				AccountUsageMeterView(
					label: account.windowLabel(seconds: account.secondaryWindowSeconds),
					remainingPercent: account.secondaryRemainingPercent,
					resetAtUnixEpoch: account.secondaryResetsAtUnixEpoch,
					tone: account.usageTone(remainingPercent: account.secondaryRemainingPercent)
				)
			}
		}
		.frame(maxWidth: .infinity)
		.padding(.horizontal, 1)
		.padding(.vertical, 1)
	}
}

struct AccountSevenDayUsageLineView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 5) {
			Image(systemName: "calendar")
				.font(PanelFont.summaryIcon)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
				.frame(width: 10)

			Text("7d used")
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)

			Text(usedText)
				.font(PanelFont.usageValue)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.92 : 0.86))
				.monospacedDigit()
				.lineLimit(1)

			if let recordDate {
				Text(recordDate)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
					.monospacedDigit()
					.lineLimit(1)
			}

			Spacer(minLength: 4)

			HStack(alignment: .firstTextBaseline, spacing: 3) {
				Text("avg")
					.font(PanelFont.usageLabel)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
					.lineLimit(1)

				Text(dailyAverageText)
					.font(PanelFont.usageValue)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.monospacedDigit()
					.lineLimit(1)
			}
		}
		.frame(height: 16)
		.accessibilityLabel("Seven day used \(usedText), daily average \(dailyAverageText)")
	}

	private var usedText: String {
		guard let used = account.sevenDayUsedPercent else {
			return "-"
		}

		return "\(used)%"
	}

	private var dailyAverageText: String {
		guard let average = account.sevenDayDailyAveragePercent else {
			return "-"
		}

		return formatDailyUsageRate(average)
	}

	private var recordDate: String? {
		account.recentUsageRecords.last.map { compactUsageDate($0.date) }
	}
}

struct AccountUsageMeterView: View {
	let label: String
	let remainingPercent: Int?
	let resetAtUnixEpoch: Int?
	let tone: AccountTone
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 3) {
			HStack(spacing: 5) {
				Text(label)
					.font(PanelFont.usageLabel)
					.frame(width: 28, alignment: .leading)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))

				Text(remainingText)
					.font(PanelFont.usageValue)
					.frame(width: 62, alignment: .leading)
					.foregroundStyle(valueColor)
					.monospacedDigit()

				Spacer(minLength: 2)

				Text(resetDisplay.short)
					.font(PanelFont.usageValue)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.82 : 0.9))
					.monospacedDigit()
					.lineLimit(1)

				if resetDisplay.date.isEmpty == false {
					Text(resetDisplay.date)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.68 : 0.78))
						.lineLimit(1)
						.truncationMode(.middle)
					}
			}
			.frame(height: 14)

			GeometryReader { proxy in
				ZStack(alignment: .leading) {
					let width = fillWidth(in: proxy.size.width)

					Capsule()
						.fill(trackColor)
						.overlay {
							Capsule()
								.fill(trackInsetStyle)
								.padding(.vertical, 0.8)
								.allowsHitTesting(false)
						}
					Capsule()
						.fill(fillStyle)
						.frame(width: width)
						.clipShape(Capsule())
						.animation(PanelMotion.state, value: remainingPercent)
						.shadow(
							color: color.opacity(colorScheme == .dark ? 0.09 : 0.07),
							radius: colorScheme == .dark ? 1.2 : 1,
							x: 0,
							y: 0
						)
					Capsule()
						.strokeBorder(trackEdgeColor, lineWidth: 0.24)
						.allowsHitTesting(false)
				}
			}
			.frame(height: 3.2)
		}
		.lineLimit(1)
		.frame(height: 22)
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel("\(label) remaining \(remainingText), \(resetDisplay.accessibility)")
	}

	private var remainingText: String {
		guard let remainingPercent else {
			return "-"
		}

		return "\(remainingPercent)% left"
	}

	private var progress: CGFloat {
		guard let remainingPercent else {
			return 0
		}

		return CGFloat(max(0, min(100, remainingPercent))) / 100
	}

	private func fillWidth(in width: CGFloat) -> CGFloat {
		guard remainingPercent != nil else {
			return 0
		}

		return max(4, width * progress)
	}

	private var color: Color {
		switch tone {
		case .codexActive: return PanelPalette.codexAccent(colorScheme)
		case .ready: return PanelPalette.capacityAccent(colorScheme)
		case .selected: return PanelPalette.routeAccent(colorScheme)
		case .warning: return PanelPalette.warning(colorScheme)
		case .danger: return PanelPalette.destructive(colorScheme)
		case .neutral: return PanelPalette.secondaryText(colorScheme)
		}
	}

	private var valueColor: Color {
		switch tone {
		case .warning, .danger:
			return color.opacity(colorScheme == .dark ? 0.95 : 0.78)
		default:
			return PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.9 : 0.84)
		}
	}

	private var resetDisplay: UsageResetDisplay {
		UsageResetDisplay.make(resetAtUnixEpoch: resetAtUnixEpoch)
	}

	private var trackColor: Color {
		PanelPalette.progressTrack(colorScheme)
	}

	private var trackEdgeColor: Color {
		PanelPalette.progressEdge(colorScheme)
	}

	private var fillStyle: LinearGradient {
		LinearGradient(
			colors: [
				color.opacity(colorScheme == .dark ? 0.78 : 0.68),
				color.opacity(colorScheme == .dark ? 0.62 : 0.52),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	private var trackInsetStyle: LinearGradient {
		LinearGradient(
			colors: [
				Color.white.opacity(colorScheme == .dark ? 0.022 : 0.05),
				Color.white.opacity(0),
				Color.black.opacity(colorScheme == .dark ? 0.035 : 0.018),
			],
			startPoint: .top,
			endPoint: .bottom
		)
	}
}

private struct UsageGlassTrackView: View {
	let progress: CGFloat
	let tint: Color
	let markers: [CGFloat]
	let alertMarker: CGFloat?
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		GeometryReader { proxy in
			let trackWidth = proxy.size.width
			let boundedProgress = max(0, min(1, progress))
			let fillWidth = max(boundedProgress > 0 ? 5 : 0, trackWidth * boundedProgress)

			ZStack(alignment: .leading) {
				Capsule()
					.fill(trackFill)
					.overlay {
						Capsule()
							.strokeBorder(PanelPalette.progressEdge(colorScheme), lineWidth: 0.4)
					}

				Capsule()
					.fill(fillFill)
					.frame(width: fillWidth)
					.shadow(color: tint.opacity(colorScheme == .dark ? 0.18 : 0.12), radius: 2, x: 0, y: 0)
					.animation(PanelMotion.state, value: progress)

				ForEach(markers, id: \.self) { marker in
					Rectangle()
						.fill(markerColor)
						.frame(width: 1.2)
						.padding(.vertical, 0.8)
						.offset(x: markerOffset(marker, in: trackWidth))
				}

				if let alertMarker {
					Rectangle()
						.fill(PanelPalette.destructive(colorScheme))
						.frame(width: 2)
						.padding(.vertical, 0.2)
						.offset(x: markerOffset(alertMarker, in: trackWidth))
				}
			}
		}
		.accessibilityHidden(true)
	}

	private var trackFill: LinearGradient {
		LinearGradient(
			colors: [
				PanelPalette.progressTrack(colorScheme).opacity(0.92),
				PanelPalette.progressTrack(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.82),
			],
			startPoint: .top,
			endPoint: .bottom
		)
	}

	private var fillFill: LinearGradient {
		LinearGradient(
			colors: [
				tint.opacity(colorScheme == .dark ? 0.9 : 0.78),
				tint.opacity(colorScheme == .dark ? 0.72 : 0.64),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	private var markerColor: Color {
		colorScheme == .dark
			? Color.white.opacity(0.48)
			: Color.white.opacity(0.76)
	}

	private func markerOffset(_ marker: CGFloat, in width: CGFloat) -> CGFloat {
		max(0, min(width - 1.2, width * max(0, min(1, marker))))
	}
}

private struct UsageResetDisplay {
	let short: String
	let date: String
	let accessibility: String

	static func make(resetAtUnixEpoch: Int?) -> UsageResetDisplay {
		guard let seconds = resetAtUnixEpoch, seconds > 0 else {
			return UsageResetDisplay(
				short: "-",
				date: "",
				accessibility: "reset unavailable"
			)
		}

		let resetAt = Date(timeIntervalSince1970: TimeInterval(seconds))
		guard resetAt.timeIntervalSince1970.isFinite else {
			return UsageResetDisplay(
				short: "unknown",
				date: "",
				accessibility: "remaining unknown"
			)
		}

		let distanceSeconds = Int(floor(resetAt.timeIntervalSinceNow))
		if distanceSeconds <= 0 {
			let date = formatResetDate(resetAt)
			return UsageResetDisplay(
				short: "0m",
				date: date,
				accessibility: "reset at \(date), reset due now"
			)
		}

		let short = formatResetDuration(distanceSeconds)
		let date = formatResetDate(resetAt)
		return UsageResetDisplay(
			short: short,
			date: date,
			accessibility: "reset at \(date), resets in \(short)"
		)
	}

	private static func formatResetDuration(_ seconds: Int) -> String {
		let value = max(0, seconds)
		if value < 60 {
			return "<1m"
		}

		let days = value / 86_400
		let hours = (value % 86_400) / 3_600
		let minutes = (value % 3_600) / 60

		if days > 0 {
			return hours > 0 ? "\(days)d \(hours)h" : "\(days)d"
		}

		if hours > 0 {
			return "\(hours)h \(minutes)m"
		}

		return "\(minutes)m"
	}

	private static func formatResetDate(_ date: Date) -> String {
		let formatter = DateFormatter()
		formatter.locale = Locale(identifier: "en_US_POSIX")
		let calendar = Calendar(identifier: .gregorian)
		formatter.dateFormat = calendar.component(.year, from: date) == calendar.component(.year, from: Date())
			? "MMM d HH:mm"
			: "MMM d yyyy HH:mm"
		return formatter.string(from: date)
	}
}

struct NoticeView: View {
	let text: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .top, spacing: 7) {
			Image(systemName: "exclamationmark.triangle")
				.foregroundStyle(PanelPalette.warning(colorScheme))
			Text(text)
				.font(PanelFont.notice)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)
		}
		.padding(8)
		.modernGlassSurface(
			cornerRadius: 9,
			depth: .section
		)
	}
}

struct OperatorStatusStripView: View {
	let snapshot: OperatorSnapshotResponse
	let updatedAt: Date?
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 5) {
			HStack(spacing: 0) {
				ForEach(Array(metrics.enumerated()), id: \.element.id) { index, metric in
					if index > 0 {
						flowDivider
					}

					OperatorFlowMetricView(metric: metric)
				}
			}
			.frame(height: 32)

			if let warning = snapshot.warningSummary {
				HStack(spacing: 5) {
					Image(systemName: "exclamationmark.circle")
						.font(PanelFont.summaryIcon)
						.foregroundStyle(PanelPalette.warning(colorScheme).opacity(0.82))
						.frame(width: 10)

					Text(warning)
						.font(PanelFont.metricLabel)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.lineLimit(1)
						.truncationMode(.tail)

					Spacer(minLength: 4)

					Text(refreshMeta)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.68))
						.monospacedDigit()
				}
				.frame(height: 16)
			}
		}
		.padding(.horizontal, 6)
		.padding(.vertical, 5)
		.frame(maxWidth: .infinity, alignment: .leading)
		.modernGlassSurface(cornerRadius: 10, depth: .section)
	}

	private var flowDivider: some View {
		Rectangle()
			.fill(PanelPalette.separator(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.9))
			.frame(width: 0.5, height: 21)
	}

	private var metrics: [OperatorFlowMetric] {
		[
			OperatorFlowMetric(
				title: "Intake",
				value: snapshot.queuedCount,
				unitSingular: "issue",
				unitPlural: "issues",
				tint: PanelPalette.secondaryText(colorScheme)
			),
			OperatorFlowMetric(
				title: "Running",
				value: snapshot.activeRunCount,
				unitSingular: "lane",
				unitPlural: "lanes",
				tint: PanelPalette.routeAccent(colorScheme)
			),
			OperatorFlowMetric(
				title: "Review",
				value: snapshot.reviewCount,
				unitSingular: "PR",
				unitPlural: "PRs",
				tint: PanelPalette.codexAccent(colorScheme)
			),
			OperatorFlowMetric(
				title: "Landing",
				value: snapshot.landingCount,
				unitSingular: "PR",
				unitPlural: "PRs",
				tint: PanelPalette.landingAccent(colorScheme)
			),
		]
	}

	private var refreshMeta: String {
		guard let updatedAt else {
			return "WS live"
		}

		let age = max(0, Int(Date().timeIntervalSince(updatedAt).rounded()))
		if age < 2 {
			return "live"
		}

		return "\(age)s ago"
	}
}

struct OperatorLanePopoverView: View {
	let run: OperatorRunStatus
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 7) {
			header

			if let projectReadout {
				OperatorLaneReadoutRow(title: "Project", items: [projectReadout])
			}

			if let modelBucket {
				OperatorLaneProgressReadoutRow(
					title: "Model",
					percent: bucketPercent(modelBucket),
					elapsed: formatActivityDuration(modelBucket.wallSeconds) ?? "0s",
					total: formatActivityDuration(totalWallSeconds) ?? "0s",
					barShare: bucketShare(modelBucket)
				)
				OperatorLaneReadoutDivider()
			}

			ForEach(detailBuckets) { bucket in
				OperatorLaneReadoutRow(title: humanizedPanelToken(bucket.name), items: bucketReadoutItems(bucket))
			}

			if contextReadoutItems.isEmpty == false {
				OperatorLaneReadoutDivider()
				OperatorLaneReadoutRow(title: "Context", items: contextReadoutItems)
			}
		}
		.padding(9)
		.modernGlassSurface(cornerRadius: 12, depth: .section)
		.accessibilityLabel("Lane activity for \(run.compactTitle)")
	}

	private var tint: Color {
		if run.hasAttentionTone {
			return PanelPalette.warning(colorScheme)
		}
		if run.isWaiting {
			return PanelPalette.secondaryText(colorScheme)
		}

		return PanelPalette.routeAccent(colorScheme)
	}

	private var activity: OperatorChildAgentActivity? {
		run.childAgentActivity
	}

	private var currentSummary: String {
			guard let activity else {
				return "Waiting for child activity"
			}

		let label = panelTrimmed(activity.currentDetail)
			?? panelTrimmed(activity.currentBucket).map(humanizedPanelToken)
			?? "Active"
		if let elapsed = formatActivityDuration(activity.currentElapsedSeconds) {
			return "\(humanizedPanelToken(label)) · \(elapsed)"
		}

		return humanizedPanelToken(label)
	}

	private var header: some View {
		OperatorLaneReadoutRow(title: "Activity", items: [
			OperatorLaneReadoutItem(label: nil, value: currentSummary, tone: .primary),
		], trailing: run.compactTitle)
	}

	private var projectReadout: OperatorLaneReadoutItem? {
		guard let projectName = panelTrimmed(run.projectDisplayName) ?? panelTrimmed(run.projectID) else {
			return nil
		}

		return OperatorLaneReadoutItem(label: nil, value: projectName, tone: .primary)
	}

	private var modelBucket: OperatorChildAgentBucket? {
		orderedBuckets.first { bucket in
			bucket.name.caseInsensitiveCompare("Model") == .orderedSame
		}
	}

	private var detailBuckets: [OperatorChildAgentBucket] {
		orderedBuckets.filter { bucket in
			bucket.name.caseInsensitiveCompare("Model") != .orderedSame
				&& bucketReadoutItems(bucket).isEmpty == false
		}
	}

	private var orderedBuckets: [OperatorChildAgentBucket] {
		bucketRows.sorted { left, right in
			let leftPriority = bucketPriority(left.name)
			let rightPriority = bucketPriority(right.name)
			if leftPriority != rightPriority {
				return leftPriority < rightPriority
			}
			if left.wallSeconds != right.wallSeconds {
				return left.wallSeconds > right.wallSeconds
			}
			if left.eventCount != right.eventCount {
				return left.eventCount > right.eventCount
			}

			return left.name < right.name
		}
	}

	private var contextReadoutItems: [OperatorLaneReadoutItem] {
		guard let activity else {
			return []
		}

		var items = [OperatorLaneReadoutItem]()
		if let current = activity.inputTokensCurrent {
			items.append(OperatorLaneReadoutItem(label: "current", value: "\(formatCompactCount(current)) tok"))
		}
		if let peak = activity.inputTokensMax, peak != activity.inputTokensCurrent {
			items.append(OperatorLaneReadoutItem(label: "peak", value: "\(formatCompactCount(peak)) tok"))
		}
		if activity.inputTokensCumulative > 0 {
			items.append(OperatorLaneReadoutItem(label: "input", value: "\(formatCompactCount(activity.inputTokensCumulative)) tok"))
		}
		if activity.toolCallCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "tool_calls", value: formatCompactCount(activity.toolCallCount)))
		}
		if let largestOutput = activity.largestToolOutputBytes, largestOutput > 0 {
			items.append(OperatorLaneReadoutItem(label: "largest output", value: formatCompactBytes(largestOutput)))
		}

		return items
	}

	private var bucketRows: [OperatorChildAgentBucket] {
		activity?.buckets ?? []
	}

	private var totalWallSeconds: Int {
		max(
			1,
			activity?.wallSeconds ?? 0,
			bucketRows.reduce(0) { $0 + max(0, $1.wallSeconds) }
			)
	}

	private func bucketReadoutItems(_ bucket: OperatorChildAgentBucket) -> [OperatorLaneReadoutItem] {
		let normalizedName = bucket.name.lowercased()
		var items = [OperatorLaneReadoutItem]()

		if normalizedName.contains("tracker"), bucket.wallSeconds > 0 {
			items.append(OperatorLaneReadoutItem(label: "wall", value: formatActivityDuration(bucket.wallSeconds) ?? "0s"))
		}
		if bucket.eventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(bucket.eventCount)))
		}
		if normalizedName.contains("protocol") {
			if bucket.inputTokens > 0 {
				items.append(OperatorLaneReadoutItem(label: "input", value: "\(formatCompactCount(bucket.inputTokens)) tok"))
			}
			if bucket.outputTokens > 0 {
				items.append(OperatorLaneReadoutItem(label: "output", value: "\(formatCompactCount(bucket.outputTokens)) tok"))
			}
		} else {
			if bucket.toolCallCount > 0 {
				items.append(OperatorLaneReadoutItem(label: "tool_calls", value: formatCompactCount(bucket.toolCallCount)))
			}
			if bucket.outputBytes > 0 {
				items.append(OperatorLaneReadoutItem(label: "output bytes", value: formatCompactBytes(bucket.outputBytes)))
			}
			if normalizedName.contains("tracker") == false {
				if bucket.inputTokens > 0 {
					items.append(OperatorLaneReadoutItem(label: "input", value: "\(formatCompactCount(bucket.inputTokens)) tok"))
				}
				if bucket.outputTokens > 0 {
					items.append(OperatorLaneReadoutItem(label: "output", value: "\(formatCompactCount(bucket.outputTokens)) tok"))
				}
			}
		}

		return items
	}

	private func bucketPriority(_ name: String) -> Int {
		let normalizedName = name.lowercased()
		if normalizedName.contains("model") {
			return 0
		}
		if normalizedName.contains("protocol") {
			return 1
		}
		if normalizedName.contains("tracker") {
			return 2
		}

		return 10
	}

	private func bucketShare(_ bucket: OperatorChildAgentBucket) -> CGFloat {
		guard bucket.wallSeconds > 0 else {
			return 0
		}

		return min(1, max(0.02, CGFloat(bucket.wallSeconds) / CGFloat(max(1, totalWallSeconds))))
	}

	private func bucketPercent(_ bucket: OperatorChildAgentBucket) -> Int {
		Int((Double(bucket.wallSeconds) / Double(max(1, totalWallSeconds)) * 100).rounded())
	}
}

struct OperatorLaneReadoutItem: Identifiable {
	enum Tone {
		case primary
		case secondary
	}

	let label: String?
	let value: String
	let tone: Tone

	init(label: String?, value: String, tone: Tone = .secondary) {
		self.label = label
		self.value = value
		self.tone = tone
	}

	var id: String {
		"\(label ?? "value")-\(value)"
	}
}

struct OperatorLaneReadoutRow: View {
	let title: String
	let items: [OperatorLaneReadoutItem]
	let trailing: String?
	@Environment(\.colorScheme) private var colorScheme

	init(title: String, items: [OperatorLaneReadoutItem], trailing: String? = nil) {
		self.title = title
		self.items = items
		self.trailing = trailing
	}

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 8) {
			Text(title)
				.font(PanelFont.lanePopoverLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
				.frame(width: 62, alignment: .leading)

			OperatorLaneReadoutFlowLayout(spacing: 8, rowSpacing: 4) {
				ForEach(items) { item in
					OperatorLaneReadoutItemView(item: item)
				}
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			if let trailing = panelTrimmed(trailing) {
				Text(trailing)
					.font(PanelFont.lanePopoverMeta)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.76))
					.lineLimit(1)
					.truncationMode(.middle)
					.frame(maxWidth: 78, alignment: .trailing)
			}
		}
	}
}

struct OperatorLaneProgressReadoutRow: View {
	let title: String
	let percent: Int
	let elapsed: String
	let total: String
	let barShare: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .center, spacing: 8) {
			Text(title)
				.font(PanelFont.lanePopoverLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
				.frame(width: 62, alignment: .leading)

			UsageGlassTrackView(
				progress: barShare,
				tint: PanelPalette.usageCyan(colorScheme),
				markers: [0.25, 0.5, 0.75],
				alertMarker: nil
			)
			.frame(height: 5.5)
			.frame(maxWidth: .infinity)

			Text("\(percent)% · \(elapsed) / \(total)")
				.font(PanelFont.lanePopoverMeta)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.monospacedDigit()
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: 22)
	}
}

struct OperatorLaneReadoutItemView: View {
	let item: OperatorLaneReadoutItem
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 3) {
			if let label = item.label {
				Text(label)
					.font(PanelFont.lanePopoverMeta)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
			}

			Text(item.value)
				.font(PanelFont.lanePopoverValue)
				.foregroundStyle(valueColor)
				.monospacedDigit()
				.lineLimit(1)
				.minimumScaleFactor(0.78)
		}
		.fixedSize(horizontal: true, vertical: false)
	}

	private var valueColor: Color {
		switch item.tone {
		case .primary:
			return PanelPalette.primaryText(colorScheme)
		case .secondary:
			return PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.9 : 0.84)
		}
	}
}

struct OperatorLaneReadoutDivider: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Rectangle()
			.fill(PanelPalette.separator(colorScheme))
			.frame(height: 0.5)
			.padding(.vertical, 1)
	}
}

struct OperatorLaneReadoutFlowLayout: Layout {
	let spacing: CGFloat
	let rowSpacing: CGFloat

	init(spacing: CGFloat = 8, rowSpacing: CGFloat = 4) {
		self.spacing = spacing
		self.rowSpacing = rowSpacing
	}

	func sizeThatFits(
		proposal: ProposedViewSize,
		subviews: Subviews,
		cache: inout Void
	) -> CGSize {
		let maxWidth = max(0, proposal.width ?? subviews.map { $0.sizeThatFits(.unspecified).width }.reduce(0, +))
		var currentX: CGFloat = 0
		var currentY: CGFloat = 0
		var rowHeight: CGFloat = 0
		var measuredWidth: CGFloat = 0

		for subview in subviews {
			let size = subview.sizeThatFits(.unspecified)
			if currentX > 0, currentX + spacing + size.width > maxWidth {
				currentY += rowHeight + rowSpacing
				currentX = 0
				rowHeight = 0
			}

			if currentX > 0 {
				currentX += spacing
			}
			currentX += size.width
			rowHeight = max(rowHeight, size.height)
			measuredWidth = max(measuredWidth, currentX)
		}

		return CGSize(width: proposal.width ?? measuredWidth, height: currentY + rowHeight)
	}

	func placeSubviews(
		in bounds: CGRect,
		proposal: ProposedViewSize,
		subviews: Subviews,
		cache: inout Void
	) {
		let maxWidth = bounds.width
		var currentX: CGFloat = bounds.minX
		var currentY: CGFloat = bounds.minY
		var rowHeight: CGFloat = 0

		for subview in subviews {
			let size = subview.sizeThatFits(.unspecified)
			if currentX > bounds.minX, currentX + spacing + size.width > bounds.minX + maxWidth {
				currentY += rowHeight + rowSpacing
				currentX = bounds.minX
				rowHeight = 0
			}

			if currentX > bounds.minX {
				currentX += spacing
			}
			subview.place(
				at: CGPoint(x: currentX, y: currentY),
				proposal: ProposedViewSize(size)
			)
			currentX += size.width
			rowHeight = max(rowHeight, size.height)
		}
	}
}

struct OperatorFlowMetric: Identifiable {
	let title: String
	let value: Int
	let unitSingular: String
	let unitPlural: String
	let tint: Color

	init(
		title: String,
		value: Int,
		unitSingular: String,
		unitPlural: String,
		tint: Color
	) {
		self.title = title
		self.value = value
		self.unitSingular = unitSingular
		self.unitPlural = unitPlural
		self.tint = tint
	}

	var id: String {
		title
	}

	var unit: String {
		value == 1 ? unitSingular : unitPlural
	}
}

struct OperatorFlowMetricView: View {
	let metric: OperatorFlowMetric
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 1) {
			Text(metric.title)
				.font(PanelFont.metricLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)

			Text("\(metric.value) \(metric.unit)")
				.font(PanelFont.metricValue)
				.foregroundStyle(valueTint)
				.monospacedDigit()
				.lineLimit(1)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(.horizontal, 5)
		.help(metric.title)
	}

	private var valueTint: Color {
		metric.value > 0
			? metric.tint
			: PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.76 : 0.66)
	}
}

struct SummaryTileView: View {
	let title: String
	let value: String
	let symbol: String
	let tint: Color
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 4) {
			Image(systemName: symbol)
				.font(PanelFont.summaryIcon)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.78 : 0.82))
				.frame(width: 11)

			Text(title)
				.font(PanelFont.metricLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)

			Text(value)
				.font(PanelFont.metricValue)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.middle)
				.layoutPriority(1)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

struct PanelIconButtonView: View {
	let symbol: String
	let tint: Color
	let isActive: Bool
	let isDestructive: Bool
	let isDisabled: Bool
	let isSubtle: Bool
	let isPrimary: Bool
	let size: CGFloat
	let action: () -> Void
	let help: String
	@Environment(\.colorScheme) private var colorScheme

	init(
		symbol: String,
		tint: Color,
		isActive: Bool,
		isDestructive: Bool = false,
		isDisabled: Bool = false,
		isSubtle: Bool = false,
		isPrimary: Bool = false,
		size: CGFloat = 24,
		action: @escaping () -> Void,
		help: String
	) {
		self.symbol = symbol
		self.tint = tint
		self.isActive = isActive
		self.isDestructive = isDestructive
		self.isDisabled = isDisabled
		self.isSubtle = isSubtle
		self.isPrimary = isPrimary
		self.size = size
		self.action = action
		self.help = help
	}

	var body: some View {
		Button(action: action) {
			buttonLabel
		}
		.buttonStyle(
			PanelInteractiveButtonStyle(
				isDisabled: isDisabled,
				hoverLift: 0,
				hoverScale: isSubtle ? 1.004 : 1.006,
				pressedScale: 0.952,
				hoverShadowRadius: isSubtle ? 2.4 : 3
			)
		)
		.disabled(isDisabled)
		.opacity(isDisabled && isActive == false ? 0.56 : 1)
		.help(help)
	}

	@ViewBuilder
	private var buttonLabel: some View {
		if usesSurface {
			iconContent
				.modernGlassSurface(
					cornerRadius: iconCornerRadius,
					depth: .control
				)
		} else {
			iconContent
				.opacity(isDisabled ? 0.34 : 0.82)
		}
	}

	private var iconContent: some View {
		Image(systemName: symbol)
			.font(PanelFont.iconButton)
			.symbolRenderingMode(.monochrome)
			.foregroundStyle(foregroundColor)
			.frame(width: size, height: size)
			.contentShape(RoundedRectangle(cornerRadius: iconCornerRadius, style: .continuous))
	}

	private var foregroundColor: Color {
		if isActive {
			return tint.opacity(colorScheme == .dark ? 0.98 : 0.92)
		}
		if isDisabled {
			return PanelPalette.secondaryText(colorScheme).opacity(0.38)
		}
		if isDestructive {
			return tint.opacity(colorScheme == .dark ? 0.96 : 0.9)
		}
		if isPrimary {
			return tint.opacity(colorScheme == .dark ? 1 : 0.96)
		}
		if isSubtle {
			return tint.opacity(colorScheme == .dark ? 0.86 : 0.82)
		}
		return PanelPalette.actionBlue(colorScheme).opacity(colorScheme == .dark ? 0.88 : 0.86)
	}

	private var usesSurface: Bool {
		if isSubtle {
			return false
		}
		if isActive || isPrimary {
			return true
		}
		return true
	}

	private var iconCornerRadius: CGFloat {
		size * 0.5
	}
}

private struct DeleteArmedShakeModifier: ViewModifier {
	let isArmed: Bool
	@State private var shakeTrigger = 0

	func body(content: Content) -> some View {
		content
			.modifier(DeleteShakeEffect(animatableData: CGFloat(shakeTrigger)))
			.scaleEffect(isArmed ? 1.045 : 1)
			.onChange(of: isArmed) { _, armed in
				guard armed else {
					return
				}
				withAnimation(.linear(duration: 0.42)) {
					shakeTrigger += 1
				}
			}
	}
}

private struct DeleteShakeEffect: GeometryEffect {
	var travel: CGFloat = 1.8
	var shakesPerUnit: CGFloat = 3
	var animatableData: CGFloat

	func effectValue(size: CGSize) -> ProjectionTransform {
		let xOffset = travel * sin(animatableData * .pi * shakesPerUnit * 2)
		return ProjectionTransform(CGAffineTransform(translationX: xOffset, y: 0))
	}
}

private enum AccountPrivacy {
	static let hiddenValue = "hidden"
	static let visibleValue = "visible"
}

private enum AccountDisplay {
	static let randomNames = [
		"Alex",
		"Avery",
		"Bailey",
		"Blake",
		"Casey",
		"Charlie",
		"Clara",
		"Dana",
		"Drew",
		"Eden",
		"Elliot",
		"Emery",
		"Evan",
		"Finley",
		"Harper",
		"Hayden",
		"Iris",
		"Jamie",
		"Jordan",
		"Kai",
		"Kendall",
		"Lane",
		"Liam",
		"Logan",
		"Mason",
		"Maya",
		"Mia",
		"Morgan",
		"Noah",
		"Nora",
		"Owen",
		"Paige",
		"Parker",
		"Quinn",
		"Reese",
		"Remy",
		"Riley",
		"Rowan",
		"Sage",
		"Sasha",
		"Sidney",
		"Taylor",
		"Theo",
		"Val",
	]

	static func alias(for account: CodexAccount) -> String {
		randomNames[preferredNameIndex(for: account)]
	}

	static func aliases(for accounts: [CodexAccount]) -> [String: String] {
		var usedNames = Set<String>()
		var aliases = [String: String]()
		let orderedAccounts = accounts.sorted { left, right in
			aliasSortKey(for: left) < aliasSortKey(for: right)
		}

		for account in orderedAccounts {
			let alias = uniqueAlias(startingAt: preferredNameIndex(for: account), usedNames: usedNames)
			usedNames.insert(alias)
			aliases[account.id] = alias
		}

		return aliases
	}

	static func alias(forIdentity identity: String) -> String {
		let seed = identity.trimmingCharacters(in: .whitespacesAndNewlines)
		let hash = identityHash(seed.isEmpty ? "account" : seed)
		let index = Int(hash % UInt32(randomNames.count))

		return randomNames[index]
	}

	static func compactEmail(_ email: String) -> String {
		let text = email.trimmingCharacters(in: .whitespacesAndNewlines)
		guard let atIndex = text.firstIndex(of: "@"), atIndex > text.startIndex else {
			return compactIdentity(text)
		}

		let local = String(text[..<atIndex])
		let domain = String(text[atIndex...])
		if local.count <= 6 {
			return "\(local)\(domain)"
		}

		return "\(local.prefix(3))...\(local.suffix(3))\(domain)"
	}

	static func compactIdentity(_ value: String) -> String {
		let text = trimLeadingEllipsis(value)
		if text.isEmpty || text == "unknown" {
			return text
		}

		let edgeLength = max(3, min(6, text.count / 2))
		return "\(text.prefix(edgeLength))...\(text.suffix(edgeLength))"
	}

	private static func trimLeadingEllipsis(_ value: String) -> String {
		let text = value.trimmingCharacters(in: .whitespacesAndNewlines)
		if text.hasPrefix("..."), text.dropFirst(3).contains("...") == false {
			return String(text.dropFirst(3))
		}

		return text
	}

	private static func identityHash(_ value: String) -> UInt32 {
		var hash: UInt32 = 2_166_136_261
		for unit in value.utf16 {
			hash ^= UInt32(unit)
			hash = hash &* 16_777_619
		}

		return hash
	}

	private static func aliasSortKey(for account: CodexAccount) -> String {
		if let key = account.randomNameKey?.trimmingCharacters(in: .whitespacesAndNewlines),
			key.isEmpty == false
		{
			return key
		}

		return account.randomNameSeed
	}

	private static func preferredNameIndex(for account: CodexAccount) -> Int {
		if let randomName = account.randomName?.trimmingCharacters(in: .whitespacesAndNewlines),
			let index = randomNames.firstIndex(of: randomName)
		{
			return index
		}

		let hash = randomNameHash(for: account)
		let offset = normalizedOffset(account.randomNameOffset ?? 0)

		return (Int(hash % UInt32(randomNames.count)) + offset) % randomNames.count
	}

	private static func randomNameHash(for account: CodexAccount) -> UInt32 {
		if let key = account.randomNameKey?.trimmingCharacters(in: .whitespacesAndNewlines),
			key.isEmpty == false,
			let hash = UInt32(key, radix: 16)
		{
			return hash
		}

		return identityHash(account.randomNameSeed)
	}

	private static func normalizedOffset(_ offset: Int) -> Int {
		((offset % randomNames.count) + randomNames.count) % randomNames.count
	}

	private static func uniqueAlias(startingAt startIndex: Int, usedNames: Set<String>) -> String {
		for probe in 0..<randomNames.count {
			let name = randomNames[(startIndex + probe) % randomNames.count]
			if usedNames.contains(name) == false {
				return name
			}
		}

		let baseName = randomNames[startIndex % randomNames.count]
		var suffix = 2
		while true {
			let name = "\(baseName) \(suffix)"
			if usedNames.contains(name) == false {
				return name
			}
			suffix += 1
		}
	}
}

private func formatUsagePercent(_ value: Double) -> String {
	guard value.isFinite else {
		return "-"
	}

	let rounded = value.rounded()
	if abs(value - rounded) < 0.05 {
		return "\(Int(rounded))%"
	}

	return String(format: "%.1f%%", value)
}

private func formatDailyUsageRate(_ value: Double) -> String {
	let percent = formatUsagePercent(value)
	guard percent != "-" else {
		return "-"
	}

	return "\(percent)/d"
}

private func formatPercentagePointDelta(_ value: Double) -> String {
	guard value.isFinite else {
		return "-"
	}

	let absValue = abs(value)
	let sign = value > 0.05 ? "+" : (value < -0.05 ? "-" : "")
	let rounded = absValue.rounded()
	if abs(absValue - rounded) < 0.05 {
		return "\(sign)\(Int(rounded))pp"
	}

	return String(format: "%@%.1fpp", sign, absValue)
}

private func compactUsageDate(_ value: String) -> String {
	let parts = value.split(separator: "-")
	guard parts.count == 3, let month = Int(parts[1]), let day = Int(parts[2]) else {
		return value
	}

	var components = DateComponents()
	components.calendar = Calendar(identifier: .gregorian)
	components.year = 2_000
	components.month = month
	components.day = day
	guard let date = components.date else {
		return value
	}

	let formatter = DateFormatter()
	formatter.locale = Locale(identifier: "en_US_POSIX")
	formatter.dateFormat = "MMM d"
	return formatter.string(from: date)
}

private func panelTrimmed(_ value: String?) -> String? {
	value?.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func humanizedPanelToken(_ value: String) -> String {
	let words = value
		.replacingOccurrences(of: "-", with: " ")
		.replacingOccurrences(of: "_", with: " ")
		.split(separator: " ")
		.map { word in
			let text = String(word)
			guard let first = text.first else {
				return text
			}

			return first.uppercased() + String(text.dropFirst())
		}

	return words.joined(separator: " ")
}

private func formatLaneDuration(_ seconds: Int?) -> String? {
	guard let seconds else {
		return nil
	}

	let value = max(0, seconds)
	if value < 60 {
		return "<1m"
	}

	let days = value / 86_400
	let hours = (value % 86_400) / 3_600
	let minutes = (value % 3_600) / 60
	if days > 0 {
		return hours > 0 ? "\(days)d \(hours)h" : "\(days)d"
	}
	if hours > 0 {
		return minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
	}

	return "\(minutes)m"
}

private func formatActivityDuration(_ seconds: Int?) -> String? {
	guard let seconds else {
		return nil
	}

	let value = max(0, seconds)
	if value < 60 {
		return "\(value)s"
	}

	let hours = value / 3_600
	let minutes = (value % 3_600) / 60
	let remainderSeconds = value % 60
	if hours > 0 {
		return minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
	}
	if minutes > 0 {
		return remainderSeconds > 0 ? "\(minutes)m \(remainderSeconds)s" : "\(minutes)m"
	}

	return "\(remainderSeconds)s"
}

private func formatCompactCount(_ value: Int) -> String {
	let absoluteValue = abs(Double(value))
	let sign = value < 0 ? "-" : ""

	if absoluteValue >= 1_000_000_000 {
		return "\(sign)\(formatCompactDecimal(absoluteValue / 1_000_000_000))B"
	}
	if absoluteValue >= 1_000_000 {
		return "\(sign)\(formatCompactDecimal(absoluteValue / 1_000_000))M"
	}
	if absoluteValue >= 1_000 {
		return "\(sign)\(formatCompactDecimal(absoluteValue / 1_000))K"
	}

	return "\(value)"
}

private func formatCompactBytes(_ value: Int) -> String {
	let absoluteValue = max(0, Double(value))
	if absoluteValue >= 1_073_741_824 {
		return "\(formatCompactDecimal(absoluteValue / 1_073_741_824))GB"
	}
	if absoluteValue >= 1_048_576 {
		return "\(formatCompactDecimal(absoluteValue / 1_048_576))MB"
	}
	if absoluteValue >= 1_024 {
		return "\(formatCompactDecimal(absoluteValue / 1_024))KB"
	}

	return "\(max(0, value))B"
}

private func formatCompactDecimal(_ value: Double) -> String {
	let rounded = (value * 10).rounded() / 10
	if rounded >= 10 || rounded.rounded() == rounded {
		return String(format: "%.0f", rounded)
	}

	return String(format: "%.1f", rounded)
}

private func formatPanelTimestamp(_ value: String?) -> String? {
	guard let value = panelTrimmed(value) else {
		return nil
	}

	let date = parsePanelTimestamp(value)
	guard let date else {
		return value
	}

	let formatter = DateFormatter()
	formatter.locale = Locale(identifier: "en_US_POSIX")
	let calendar = Calendar(identifier: .gregorian)
	formatter.dateFormat = calendar.component(.year, from: date) == calendar.component(.year, from: Date())
		? "MMM d HH:mm"
		: "MMM d yyyy HH:mm"
	return formatter.string(from: date)
}

private func parsePanelTimestamp(_ value: String) -> Date? {
	let fractionalFormatter = ISO8601DateFormatter()
	fractionalFormatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
	if let date = fractionalFormatter.date(from: value) {
		return date
	}

	return ISO8601DateFormatter().date(from: value)
}

private func compactLanePath(_ value: String?) -> String? {
	guard var text = panelTrimmed(value), text.isEmpty == false else {
		return nil
	}

	if let home = ProcessInfo.processInfo.environment["HOME"], home.isEmpty == false {
		text = text.replacingOccurrences(of: home, with: "~")
	}
	if text.count <= 42 {
		return text
	}

	let prefix = text.prefix(18)
	let suffix = text.suffix(20)
	return "\(prefix)...\(suffix)"
}

enum GlassSurfaceDepth {
	case panel
	case section
	case row
	case control
}

extension View {
	func modernGlassSurface(
		cornerRadius: CGFloat,
		depth: GlassSurfaceDepth = .section
	) -> some View {
		modifier(
			ModernGlassSurfaceModifier(
				cornerRadius: cornerRadius,
				depth: depth
			)
		)
	}
}

struct ModernGlassSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	let cornerRadius: CGFloat
	let depth: GlassSurfaceDepth

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		if #available(macOS 26.0, *) {
			content
				.glassEffect(
					configuredGlass,
					in: shape
				)
				.overlay {
					shape
						.strokeBorder(surfaceStroke, lineWidth: strokeWidth)
						.allowsHitTesting(false)
				}
				.shadow(
					color: surfaceShadow,
					radius: shadowRadius,
					x: 0,
					y: shadowY
				)
		} else {
			content
				.background {
					shape.fill(materialStyle)
				}
				.overlay {
					shape
						.strokeBorder(surfaceStroke, lineWidth: strokeWidth)
						.allowsHitTesting(false)
				}
				.shadow(
					color: surfaceShadow,
					radius: shadowRadius,
					x: 0,
					y: shadowY
				)
		}
	}

	@available(macOS 26.0, *)
	private var configuredGlass: Glass {
		var glass = Glass.regular.tint(glassTint)
		if depth == .control {
			glass = glass.interactive()
		}

		return glass
	}

	private var glassTint: Color? {
		switch depth {
		case .panel:
			return colorScheme == .dark
				? Color(red: 0.66, green: 0.74, blue: 0.86).opacity(0.06)
				: Color.white.opacity(0.05)
		case .section:
			return colorScheme == .dark
				? Color(red: 0.72, green: 0.8, blue: 0.92).opacity(0.1)
				: Color.white.opacity(0.08)
		case .row:
			return colorScheme == .dark
				? Color(red: 0.7, green: 0.78, blue: 0.9).opacity(0.08)
				: Color.white.opacity(0.06)
		case .control:
			return colorScheme == .dark
				? Color(red: 0.78, green: 0.86, blue: 0.98).opacity(0.14)
				: Color.white.opacity(0.11)
		}
	}

	private var materialStyle: AnyShapeStyle {
		switch depth {
		case .panel:
			return AnyShapeStyle(.ultraThinMaterial)
		case .section:
			return AnyShapeStyle(.thinMaterial)
		case .row:
			return colorScheme == .dark ? AnyShapeStyle(.thinMaterial) : AnyShapeStyle(.ultraThinMaterial)
		case .control:
			return colorScheme == .dark ? AnyShapeStyle(.thinMaterial) : AnyShapeStyle(.ultraThinMaterial)
		}
	}

	private var surfaceStroke: Color {
		switch depth {
		case .panel:
			return PanelPalette.glassStroke(colorScheme)
		case .section:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.82 : 0.72)
		case .row:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.62 : 0.58)
		case .control:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.55 : 0.5)
		}
	}

	private var strokeWidth: CGFloat {
		depth == .panel ? 0.8 : 0.55
	}

	private var surfaceShadow: Color {
		switch depth {
		case .panel:
			return PanelPalette.glassInnerShadow(colorScheme)
		case .section:
			return PanelPalette.glassInnerShadow(colorScheme).opacity(0.72)
		case .row:
			return PanelPalette.glassInnerShadow(colorScheme).opacity(0.5)
		case .control:
			return PanelPalette.glassInnerShadow(colorScheme).opacity(0.34)
		}
	}

	private var shadowRadius: CGFloat {
		switch depth {
		case .panel:
			return 18
		case .section:
			return 9
		case .row:
			return 5
		case .control:
			return 3
		}
	}

	private var shadowY: CGFloat {
		switch depth {
		case .panel:
			return 10
		case .section:
			return 5
		case .row:
			return 2
		case .control:
			return 1
		}
	}
}

private extension CodexAccount {
	var randomNameSeed: String {
		if accountFingerprint.isEmpty == false {
			return accountFingerprint
		}
		if let email, email.isEmpty == false {
			return email
		}
		if let planType, planType.isEmpty == false {
			return planType
		}

		return "account"
	}

	func panelDisplayName(emailsHidden: Bool) -> String {
		if emailsHidden {
			if let randomName = randomName?.trimmingCharacters(in: .whitespacesAndNewlines),
				randomName.isEmpty == false
			{
				return randomName
			}

			return AccountDisplay.alias(for: self)
		}

		return AccountDisplay.compactEmail(displayName)
	}

	func statusDisplayColor(colorScheme: ColorScheme) -> Color {
		switch statusTone {
		case .codexActive:
			return PanelPalette.codexAccent(colorScheme)
		case .ready:
			return PanelPalette.secondaryText(colorScheme)
		case .selected:
			return PanelPalette.routeAccent(colorScheme)
		case .warning:
			return PanelPalette.warning(colorScheme)
		case .danger:
			return PanelPalette.destructive(colorScheme)
		case .neutral:
			return PanelPalette.secondaryText(colorScheme)
		}
	}

	var hasPrimaryUsageData: Bool {
		primaryRemainingPercent != nil || primaryWindowSeconds != nil || primaryResetsAtUnixEpoch != nil
	}

	var hasSecondaryUsageData: Bool {
		secondaryRemainingPercent != nil || secondaryWindowSeconds != nil || secondaryResetsAtUnixEpoch != nil
	}

	var hasUsageSummary: Bool {
		hasUsageWindowSummary || hasSevenDayUsageEstimate
	}

	var hasUsageWindowSummary: Bool {
		hasPrimaryUsageData || hasSecondaryUsageData
	}

	var hasSevenDayUsageEstimate: Bool {
		sevenDayUsedPercent != nil || sevenDayDailyAveragePercent != nil
	}

	var recentUsageRecords: [AccountUsageRecord] {
		usageRecords ?? []
	}

	var compactHealthLabel: String? {
		if isUsageLimited {
			return "Limited"
		}

		switch status {
		case "available":
			return nil
		case "usage_limited":
			return "Limited"
		case "probe_failed":
			return "-"
		case "expired":
			return "Refresh needed"
		case "disabled":
			return "Disabled"
		case "cooldown":
			return "Cooling"
		case "unusable":
			return "Needs login"
		default:
			let label = status.replacingOccurrences(of: "_", with: " ").capitalized
			return label.isEmpty ? nil : label
		}
	}
}
