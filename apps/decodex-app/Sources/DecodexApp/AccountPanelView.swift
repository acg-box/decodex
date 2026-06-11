import AppKit
import Combine
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
			? Color.white.opacity(0.14)
			: Color(red: 0.34, green: 0.42, blue: 0.52).opacity(0.24)
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
	@State private var currentTime = Date()
	@State private var pendingLogout: CodexAccount?
	@State private var armedLogoutAccountID: String?
	@State private var logoutArmToken = UUID()
	@AppStorage("decodex.operator.accountPrivacy") private var accountPrivacy = AccountPrivacy.hiddenValue
	private let localClock = Timer.publish(every: 1, on: .main, in: .common).autoconnect()

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
		.onReceive(localClock) { tick in
			currentTime = tick
		}
	}

	private var panelContent: some View {
		VStack(alignment: .leading, spacing: 6) {
			header
			accountSummary

			if telemetryMatrixIsVisible {
				AccountTelemetryMatrixView(
					aggregate: accountProfileAggregate,
					usageEstimate: store.accountList?.usageEstimate,
					accounts: store.accounts
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
		HStack(alignment: .firstTextBaseline, spacing: 7) {
			SummaryTileView(
				title: "Codex",
				value: codexAuthLabel,
				symbol: "terminal",
				tint: PanelPalette.codexAccent(colorScheme)
			)

			Rectangle()
				.fill(PanelPalette.separator(colorScheme))
				.frame(width: 0.5, height: 16)
				.alignmentGuide(.firstTextBaseline) { dimensions in
					dimensions[VerticalAlignment.center] + 4
				}

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
					currentTime: currentTime,
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

		return control.mode
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

		if telemetryMatrixIsVisible {
			height += AccountPanelLayout.sectionSpacing + telemetryMatrixHeight
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

	private var accountProfileAggregate: AccountProfileAggregate? {
		AccountProfileAggregate.make(accounts: store.accounts)
	}

	private var telemetryMatrixIsVisible: Bool {
		accountProfileAggregate != nil
			|| store.accountList?.usageEstimate != nil
	}

	private var telemetryMatrixHeight: CGFloat {
		var rows = [CGFloat]()
		if accountProfileAggregate != nil {
			rows.append(AccountPanelLayout.telemetryProfileHeight)
		}
		if let estimate = store.accountList?.usageEstimate {
			rows.append(
				estimate.accountEstimateCount < estimate.accountCount
					? AccountPanelLayout.telemetryPoolMeasuredHeight
					: AccountPanelLayout.telemetryPoolHeight
			)
		}
		guard rows.isEmpty == false else {
			return 0
		}

		return AccountPanelLayout.telemetryVerticalPadding
			+ rows.reduce(0, +)
			+ CGFloat(rows.count - 1) * AccountPanelLayout.telemetryRowSpacing
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
			base = 102
		} else {
			base = 48
		}
		let runSignal: CGFloat = operatorRuns(for: account).isEmpty ? 0 : 22
		let profileSignal: CGFloat = account.hasProfileSummary
			? (account.recentProfileDailyUsage.isEmpty ? 19 : 35)
			: 0

		return base + runSignal + profileSignal
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
	let currentTime: Date
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

					if let capacityLabel = account.currentCapacityLabel {
						Text("·")
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.62))
							.fixedSize(horizontal: true, vertical: false)

						Text(capacityLabel)
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))
							.lineLimit(1)
							.fixedSize(horizontal: true, vertical: false)
					}

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
							help: loginHelp
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
				AccountRunSummaryView(runs: runs, currentTime: currentTime)
			}

			if account.hasUsageSummary {
				AccountUsageSummaryView(account: account, currentTime: currentTime)
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
			return "Sign in again before routing runs"
		}
		if account.disabled {
			return "Disabled account cannot route runs"
		}

		return "Route Decodex runs here"
	}

	private var loginHelp: String {
		if account.recoveryActionKind == .login {
			return "Refresh token was rejected; sign in again"
		}

		return "Login account"
	}
}

struct AccountRunSummaryView: View {
	let runs: [OperatorRunStatus]
	let currentTime: Date
	@State private var placementStore = AccountRunStripPlacementStore()
	@State private var scrollProxy = AccountRunStripScrollProxy()
	@State private var scrollMetrics = AccountRunStripMetrics()
	@State private var showsEdgeControls = false

	var body: some View {
		HStack(spacing: AccountRunStripLayout.edgeControlSpacing) {
			if showsEdgeControls {
				AccountRunStripEdgeButton(
					direction: .backward,
					isEnabled: scrollMetrics.canScrollBackward
				) {
					scrollProxy.scrollToAdjacentRun(.backward)
				} startContinuousAction: {
					scrollProxy.startContinuousScroll(.backward)
				} stopContinuousAction: {
					scrollProxy.stopContinuousScroll()
				}
			}

			AccountRunStripScrollView(
				placementStore: placementStore,
				scrollProxy: scrollProxy,
				onMetricsChange: { metrics in
					updateScrollMetrics(metrics)
				}
			) {
				HStack(spacing: 5) {
					ForEach(runs) { run in
						AccountRunChipView(run: run, currentTime: currentTime)
							.modifier(
								AccountRunChipPlacementReporter(
									runID: run.id,
									placementStore: placementStore
								)
							)
					}
				}
				.padding(.trailing, 1)
				.fixedSize(horizontal: true, vertical: false)
				.coordinateSpace(name: AccountRunStripLayout.contentCoordinateSpace)
			}
			.mask {
				AccountRunStripFadeMask(metrics: showsEdgeControls ? scrollMetrics : AccountRunStripMetrics())
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			if showsEdgeControls {
				AccountRunStripEdgeButton(
					direction: .forward,
					isEnabled: scrollMetrics.canScrollForward
				) {
					scrollProxy.scrollToAdjacentRun(.forward)
				} startContinuousAction: {
					scrollProxy.startContinuousScroll(.forward)
				} stopContinuousAction: {
					scrollProxy.stopContinuousScroll()
				}
			}
		}
		.frame(height: AccountRunChipLayout.height)
		.frame(maxWidth: .infinity, alignment: .leading)
		.contentShape(Rectangle())
		.accessibilityLabel("\(runs.count) active lane\(runs.count == 1 ? "" : "s")")
		.onAppear {
			placementStore.retainOnly(Set(runs.map(\.id)))
		}
		.onChange(of: runs.map(\.id)) { _, runIDs in
			placementStore.retainOnly(Set(runIDs))
		}
	}

	private func updateScrollMetrics(_ metrics: AccountRunStripMetrics) {
		let nextShowsEdgeControls = shouldShowEdgeControls(for: metrics)
		guard metrics != scrollMetrics || nextShowsEdgeControls != showsEdgeControls else {
			return
		}

		if showsEdgeControls && nextShowsEdgeControls == false {
			scrollProxy.stopContinuousScroll()
		}

		var transaction = Transaction()
		transaction.disablesAnimations = true
		withTransaction(transaction) {
			scrollMetrics = metrics
			showsEdgeControls = nextShowsEdgeControls
		}
	}

	private func shouldShowEdgeControls(for metrics: AccountRunStripMetrics) -> Bool {
		let reservedWidth = showsEdgeControls ? AccountRunStripLayout.edgeControlReservedWidth : 0
		let viewportWidthWithoutEdgeControls = metrics.viewportWidth + reservedWidth

		return metrics.contentWidth > viewportWidthWithoutEdgeControls + AccountRunStripLayout.overflowTolerance
	}
}

private enum AccountRunStripLayout {
	static let contentCoordinateSpace = "account-run-strip-content"
	static let dragActivationDistance: CGFloat = 1
	static let edgeControlSpacing: CGFloat = 4
	static let edgeControlWidth: CGFloat = 12
	static let edgeControlReservedWidth = edgeControlWidth * 2 + edgeControlSpacing * 2
	static let fadeWidth: CGFloat = 24
	static let overflowTolerance: CGFloat = 1
	static let wheelLineDeltaScale: CGFloat = 11
	static let wheelMinimumDelta: CGFloat = 0.1
	static let clickScrollDuration: TimeInterval = 0.14
	static let continuousScrollStartDelayNanoseconds: UInt64 = 200_000_000
	static let continuousScrollTickInterval: TimeInterval = 1.0 / 120.0
	static let continuousScrollMaximumFrameInterval: TimeInterval = 1.0 / 20.0
	static let continuousScrollVelocity: CGFloat = 285
}

private enum AccountRunStripScrollDirection {
	case backward
	case forward

	var scrollMultiplier: CGFloat {
		switch self {
		case .backward:
			return -1
		case .forward:
			return 1
		}
	}

	var symbol: String {
		switch self {
		case .backward:
			return "chevron.left"
		case .forward:
			return "chevron.right"
		}
	}

	var accessibilityLabel: String {
		switch self {
		case .backward:
			return "Previous running lane"
		case .forward:
			return "Next running lane"
		}
	}

	var disabledHelp: String {
		switch self {
		case .backward:
			return "Already at the first running lane"
		case .forward:
			return "Already at the last running lane"
		}
	}
}

private struct AccountRunStripMetrics: Equatable {
	var contentWidth: CGFloat = 0
	var viewportWidth: CGFloat = 0
	var isOverflowing = false
	var canScrollBackward = false
	var canScrollForward = false

	init() {}

	init(contentWidth: CGFloat, viewportWidth: CGFloat, offsetX: CGFloat) {
		self.contentWidth = contentWidth
		self.viewportWidth = viewportWidth
		let maxOffsetX = max(0, contentWidth - viewportWidth)
		isOverflowing = contentWidth > viewportWidth + AccountRunStripLayout.overflowTolerance
		canScrollBackward = isOverflowing && offsetX > 1
		canScrollForward = isOverflowing && offsetX < maxOffsetX - 1
	}
}

private final class AccountRunStripPlacementStore {
	private var framesByRunID = [String: CGRect]()

	func update(runID: String, frame: CGRect) {
		framesByRunID[runID] = frame
	}

	func retainOnly(_ runIDs: Set<String>) {
		framesByRunID = framesByRunID.filter { runIDs.contains($0.key) }
	}

	func frame(for runID: String) -> CGRect? {
		framesByRunID[runID]
	}

	func orderedFrames() -> [CGRect] {
		framesByRunID.values.sorted { left, right in
			if left.minX == right.minX {
				return left.width < right.width
			}

			return left.minX < right.minX
		}
	}

	func runID(containing point: NSPoint) -> String? {
		framesByRunID.first { _, frame in
			frame.contains(point)
		}?.key
	}
}

private struct AccountRunChipPlacementReporter: ViewModifier {
	let runID: String
	let placementStore: AccountRunStripPlacementStore

	func body(content: Content) -> some View {
		content.background {
			GeometryReader { proxy in
				Color.clear
					.onAppear {
						publish(proxy.frame(in: .named(AccountRunStripLayout.contentCoordinateSpace)))
					}
					.onChange(of: proxy.frame(in: .named(AccountRunStripLayout.contentCoordinateSpace))) { _, frame in
						publish(frame)
					}
			}
		}
	}

	private func publish(_ frame: CGRect) {
		DispatchQueue.main.async {
			placementStore.update(runID: runID, frame: frame)
		}
	}
}

private struct AccountRunStripFadeMask: View {
	let metrics: AccountRunStripMetrics

	var body: some View {
		HStack(spacing: 0) {
			if metrics.canScrollBackward {
				LinearGradient(
					colors: [.clear, .black],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: AccountRunStripLayout.fadeWidth)
			}

			Color.black

			if metrics.canScrollForward {
				LinearGradient(
					colors: [.black, .clear],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: AccountRunStripLayout.fadeWidth)
			}
		}
	}
}

private struct AccountRunStripEdgeButton: View {
	let direction: AccountRunStripScrollDirection
	let isEnabled: Bool
	let clickAction: () -> Void
	let startContinuousAction: () -> Void
	let stopContinuousAction: () -> Void
	@Environment(\.colorScheme) private var colorScheme
	@State private var isPressed = false
	@State private var pressTask: Task<Void, Never>?

	var body: some View {
		Image(systemName: direction.symbol)
			.font(.system(size: 10.5, weight: .semibold))
			.symbolRenderingMode(.monochrome)
			.foregroundStyle(tint)
			.scaleEffect(isEnabled && isPressed ? 0.92 : 1)
		.frame(
			width: AccountRunStripLayout.edgeControlWidth,
			height: AccountRunChipLayout.height
		)
		.contentShape(Rectangle())
		.allowsHitTesting(isEnabled)
		.highPriorityGesture(
			DragGesture(minimumDistance: 0)
				.onChanged { _ in
					startPress()
				}
				.onEnded { _ in
					endPress()
				}
		)
		.onDisappear {
			cancelPress()
		}
		.onChange(of: isEnabled) { _, isEnabled in
			if isEnabled == false {
				cancelPress()
			}
		}
		.help(isEnabled ? direction.accessibilityLabel : direction.disabledHelp)
		.accessibilityLabel(direction.accessibilityLabel)
		.accessibilityValue(isEnabled ? "Available" : "Unavailable")
	}

	private var tint: Color {
		let opacity: Double
		if isEnabled == false {
			opacity = colorScheme == .dark ? 0.28 : 0.22
		} else if isPressed {
			opacity = 0.92
		} else {
			opacity = colorScheme == .dark ? 0.62 : 0.5
		}

		return PanelPalette.primaryText(colorScheme).opacity(opacity)
	}

	private func startPress() {
		guard isEnabled, pressTask == nil else {
			return
		}

		isPressed = true
		clickAction()
		pressTask = Task {
			try? await Task.sleep(nanoseconds: AccountRunStripLayout.continuousScrollStartDelayNanoseconds)
			guard Task.isCancelled == false else {
				return
			}

			await MainActor.run {
				startContinuousAction()
			}
		}
	}

	private func endPress() {
		cancelPress()
	}

	private func cancelPress() {
		pressTask?.cancel()
		pressTask = nil
		stopContinuousAction()
		isPressed = false
	}
}

private struct AccountRunStripScrollView<Content: View>: NSViewRepresentable {
	let placementStore: AccountRunStripPlacementStore
	let scrollProxy: AccountRunStripScrollProxy
	let onMetricsChange: (AccountRunStripMetrics) -> Void
	@ViewBuilder let content: () -> Content

	func makeCoordinator() -> Coordinator {
		Coordinator(onMetricsChange: onMetricsChange)
	}

	func makeNSView(context: Context) -> AccountRunStripContainerView<Content> {
		let view = AccountRunStripContainerView(
			rootView: content(),
			placementStore: placementStore
		)
		scrollProxy.attach(view)
		view.onMetricsChange = { metrics in
			context.coordinator.publish(metrics)
		}

		return view
	}

	func updateNSView(_ nsView: AccountRunStripContainerView<Content>, context: Context) {
		context.coordinator.onMetricsChange = onMetricsChange
		scrollProxy.attach(nsView)
		nsView.onMetricsChange = { metrics in
			context.coordinator.publish(metrics)
		}
		nsView.update(rootView: content())
	}

	final class Coordinator {
		var onMetricsChange: (AccountRunStripMetrics) -> Void
		private var lastMetrics: AccountRunStripMetrics?

		init(onMetricsChange: @escaping (AccountRunStripMetrics) -> Void) {
			self.onMetricsChange = onMetricsChange
		}

		@MainActor
		func publish(_ metrics: AccountRunStripMetrics) {
			guard metrics != lastMetrics else {
				return
			}

			lastMetrics = metrics
			DispatchQueue.main.async { [onMetricsChange] in
				onMetricsChange(metrics)
			}
		}
	}
}

@MainActor
private protocol AccountRunStripScrollable: AnyObject {
	func scrollToAdjacentRun(_ direction: AccountRunStripScrollDirection)
	func startContinuousScroll(_ direction: AccountRunStripScrollDirection)
	func stopContinuousScroll()
}

@MainActor
private final class AccountRunStripScrollProxy {
	private weak var target: (any AccountRunStripScrollable)?

	func attach(_ target: any AccountRunStripScrollable) {
		self.target = target
	}

	func scrollToAdjacentRun(_ direction: AccountRunStripScrollDirection) {
		target?.scrollToAdjacentRun(direction)
	}

	func startContinuousScroll(_ direction: AccountRunStripScrollDirection) {
		target?.startContinuousScroll(direction)
	}

	func stopContinuousScroll() {
		target?.stopContinuousScroll()
	}
}

private final class AccountRunStripNSScrollView: NSScrollView {
	var onScrollWheelEvent: ((NSEvent) -> Bool)?

	override func scrollWheel(with event: NSEvent) {
		if onScrollWheelEvent?(event) == true {
			return
		}

		super.scrollWheel(with: event)
	}
}

private final class AccountRunStripContainerView<Content: View>: NSView, AccountRunStripScrollable {
	private let scrollView = AccountRunStripNSScrollView()
	private let notifyingClipView = AccountRunStripClipView()
	private let continuousScroller = AccountRunContinuousScroller()
	private let hostingView: AccountRunDragHostingView<Content>
	private let placementStore: AccountRunStripPlacementStore
	private var measuredContentWidth: CGFloat = 0
	var onMetricsChange: ((AccountRunStripMetrics) -> Void)?

	init(rootView: Content, placementStore: AccountRunStripPlacementStore) {
		self.placementStore = placementStore
		hostingView = AccountRunDragHostingView(rootView: rootView)

		super.init(frame: .zero)

		scrollView.contentView = notifyingClipView
		scrollView.drawsBackground = false
		scrollView.borderType = .noBorder
		scrollView.hasHorizontalScroller = false
		scrollView.hasVerticalScroller = false
		scrollView.autohidesScrollers = true
		scrollView.scrollerStyle = .overlay
		scrollView.horizontalScrollElasticity = .none
		scrollView.verticalScrollElasticity = .none
		scrollView.onScrollWheelEvent = { [weak self] event in
			self?.handleScrollWheel(event) ?? false
		}
		notifyingClipView.onBoundsChange = { [weak self] in
			self?.publishMetrics()
		}

		hostingView.dragScrollView = scrollView
		hostingView.onDragScroll = { [weak self] in
			self?.publishMetrics()
		}
		hostingView.onClick = { [weak self] point in
			self?.scrollClickedRunToLeadingEdge(at: point)
		}

		scrollView.documentView = hostingView
		addSubview(scrollView)
	}

	@available(*, unavailable)
	required init?(coder: NSCoder) {
		fatalError("init(coder:) has not been implemented")
	}

	override var isFlipped: Bool {
		true
	}

	var clipView: NSClipView {
		scrollView.contentView
	}

	var currentMetrics: AccountRunStripMetrics {
		AccountRunStripMetrics(
			contentWidth: measuredContentWidth,
			viewportWidth: clipView.bounds.width,
			offsetX: clipView.bounds.origin.x
		)
	}

	override func layout() {
		super.layout()

		scrollView.frame = bounds
		updateDocumentFrame()
		clampScrollOffset()
		publishMetrics()
		hostingView.window?.invalidateCursorRects(for: hostingView)
	}

	func update(rootView: Content) {
		hostingView.rootView = rootView
		hostingView.invalidateIntrinsicContentSize()
		needsLayout = true
		layoutSubtreeIfNeeded()
		publishMetrics()
	}

	func scroll(by distance: CGFloat) {
		scroll(to: clipView.bounds.origin.x + distance)
	}

	func scrollToAdjacentRun(_ direction: AccountRunStripScrollDirection) {
		layoutSubtreeIfNeeded()

		guard let offset = adjacentRunOffset(for: direction) else {
			return
		}

		scroll(to: offset, animated: true)
	}

	func startContinuousScroll(_ direction: AccountRunStripScrollDirection) {
		layoutSubtreeIfNeeded()

		guard measuredContentWidth > clipView.bounds.width + 1 else {
			return
		}

		continuousScroller.start { [weak self] elapsedTime in
			guard let self else {
				return false
			}

			let previousOffset = self.clipView.bounds.origin.x
			let distance = direction.scrollMultiplier
				* AccountRunStripLayout.continuousScrollVelocity
				* CGFloat(elapsedTime)
			self.scroll(by: distance)

			return previousOffset != self.clipView.bounds.origin.x
		}
	}

	func stopContinuousScroll() {
		continuousScroller.stop()
	}

	private func handleScrollWheel(_ event: NSEvent) -> Bool {
		layoutSubtreeIfNeeded()

		guard measuredContentWidth > clipView.bounds.width + 1 else {
			return false
		}

		let distance = wheelScrollDistance(from: event)
		guard abs(distance) > AccountRunStripLayout.wheelMinimumDelta else {
			return false
		}

		let previousOffset = clipView.bounds.origin.x
		scroll(by: distance)

		return previousOffset != clipView.bounds.origin.x
	}

	private func scrollClickedRunToLeadingEdge(at point: NSPoint) {
		layoutSubtreeIfNeeded()

		guard
			measuredContentWidth > clipView.bounds.width + 1,
			let runID = placementStore.runID(containing: point),
			let frame = placementStore.frame(for: runID)
		else {
			return
		}

		scroll(to: frame.minX, animated: true)
	}

	private func adjacentRunOffset(for direction: AccountRunStripScrollDirection) -> CGFloat? {
		let maxOffset = max(0, measuredContentWidth - clipView.bounds.width)
		guard maxOffset > 0 else {
			return nil
		}

		let currentOffset = clipView.bounds.origin.x
		let orderedOffsets = placementStore.orderedFrames().map(\.minX)
		let targetOffset: CGFloat?
		switch direction {
		case .backward:
			targetOffset = orderedOffsets.last { offset in
				offset < currentOffset - 1
			} ?? (currentOffset > 0 ? 0 : nil)
		case .forward:
			targetOffset = orderedOffsets.first { offset in
				offset > currentOffset + 1
			} ?? (currentOffset < maxOffset ? maxOffset : nil)
		}

		return targetOffset.map { min(max(0, $0), maxOffset) }
	}

	private func wheelScrollDistance(from event: NSEvent) -> CGFloat {
		let rawDeltaX = event.scrollingDeltaX == 0 ? event.deltaX : event.scrollingDeltaX
		let rawDeltaY = event.scrollingDeltaY == 0 ? event.deltaY : event.scrollingDeltaY
		let deltaX = scaledWheelDelta(rawDeltaX, isPrecise: event.hasPreciseScrollingDeltas)
		let deltaY = scaledWheelDelta(rawDeltaY, isPrecise: event.hasPreciseScrollingDeltas)
		let dominantDelta = abs(deltaX) >= abs(deltaY) ? deltaX : deltaY

		return -dominantDelta
	}

	private func scaledWheelDelta(_ delta: CGFloat, isPrecise: Bool) -> CGFloat {
		isPrecise ? delta : delta * AccountRunStripLayout.wheelLineDeltaScale
	}

	private func updateDocumentFrame() {
		let contentSize = hostingView.fittingSize
		let height = max(bounds.height, AccountRunChipLayout.height)
		measuredContentWidth = max(0, ceil(contentSize.width))
		let documentWidth = max(measuredContentWidth, 1)

		hostingView.frame = NSRect(
			x: 0,
			y: 0,
			width: documentWidth,
			height: height
		)
	}

	private func clampScrollOffset() {
		let maxOffset = max(0, measuredContentWidth - clipView.bounds.width)
		let currentOffset = clipView.bounds.origin.x
		let clampedOffset = min(max(0, currentOffset), maxOffset)

		guard clampedOffset != currentOffset else {
			return
		}

		scroll(to: clampedOffset)
	}

	private func scroll(to offset: CGFloat, animated: Bool = false) {
		layoutSubtreeIfNeeded()

		let maxOffset = max(0, measuredContentWidth - clipView.bounds.width)
		let clampedOffset = min(max(0, offset), maxOffset)
		guard clampedOffset != clipView.bounds.origin.x else {
			return
		}

		if animated {
			animateScroll(to: clampedOffset)
			return
		}

		clipView.scroll(to: NSPoint(x: clampedOffset, y: clipView.bounds.origin.y))
		scrollView.reflectScrolledClipView(clipView)
		publishMetrics()
	}

	private func animateScroll(to offset: CGFloat) {
		NSAnimationContext.runAnimationGroup { context in
			context.duration = AccountRunStripLayout.clickScrollDuration
			context.allowsImplicitAnimation = true
			clipView.animator().setBoundsOrigin(NSPoint(x: offset, y: clipView.bounds.origin.y))
		}
		scrollView.reflectScrolledClipView(clipView)
		publishMetrics()
	}

	private func publishMetrics() {
		onMetricsChange?(currentMetrics)
	}
}

private final class AccountRunContinuousScroller {
	private var frameAction: ((TimeInterval) -> Bool)?
	private var lastTickTime: TimeInterval?
	private var timer: Timer?
	private var timerTarget: AccountRunContinuousTimerTarget?

	deinit {
		stop()
	}

	func start(_ frameAction: @escaping (TimeInterval) -> Bool) {
		stop()
		self.frameAction = frameAction
		lastTickTime = ProcessInfo.processInfo.systemUptime

		let timerTarget = AccountRunContinuousTimerTarget(scroller: self)
		let timer = Timer(
			timeInterval: AccountRunStripLayout.continuousScrollTickInterval,
			target: timerTarget,
			selector: #selector(AccountRunContinuousTimerTarget.timerDidFire(_:)),
			userInfo: nil,
			repeats: true
		)
		self.timerTarget = timerTarget
		self.timer = timer
		RunLoop.main.add(timer, forMode: .common)
	}

	func stop() {
		timer?.invalidate()
		timer = nil
		timerTarget = nil
		frameAction = nil
		lastTickTime = nil
	}

	fileprivate func performFrame() {
		guard let frameAction else {
			return
		}

		let tickTime = ProcessInfo.processInfo.systemUptime
		let elapsedTime = lastTickTime.map { tickTime - $0 }
			?? AccountRunStripLayout.continuousScrollTickInterval
		lastTickTime = tickTime

		let boundedElapsedTime = min(
			max(elapsedTime, 0),
			AccountRunStripLayout.continuousScrollMaximumFrameInterval
		)
		if frameAction(boundedElapsedTime) == false {
			stop()
		}
	}
}

private final class AccountRunContinuousTimerTarget: NSObject {
	weak var scroller: AccountRunContinuousScroller?

	init(scroller: AccountRunContinuousScroller) {
		self.scroller = scroller
	}

	@objc func timerDidFire(_ timer: Timer) {
		scroller?.performFrame()
	}
}

private final class AccountRunStripClipView: NSClipView {
	var onBoundsChange: (() -> Void)?

	override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
		var constrainedBounds = super.constrainBoundsRect(proposedBounds)
		constrainedBounds.origin.x = max(0, constrainedBounds.origin.x)
		constrainedBounds.origin.y = max(0, constrainedBounds.origin.y)

		return constrainedBounds
	}

	override func scroll(to newOrigin: NSPoint) {
		let oldOrigin = bounds.origin

		super.scroll(to: newOrigin)
		publishIfNeeded(from: oldOrigin)
	}

	override func setBoundsOrigin(_ newOrigin: NSPoint) {
		let oldOrigin = bounds.origin

		super.setBoundsOrigin(newOrigin)
		publishIfNeeded(from: oldOrigin)
	}

	private func publishIfNeeded(from oldOrigin: NSPoint) {
		guard oldOrigin != bounds.origin else {
			return
		}

		onBoundsChange?()
	}
}

private final class AccountRunDragHostingView<Content: View>: NSHostingView<Content> {
	weak var dragScrollView: NSScrollView?
	var onDragScroll: (() -> Void)?
	var onClick: ((NSPoint) -> Void)?
	private var dragStartPoint: NSPoint?
	private var dragStartOffset: CGFloat = 0
	private var isDraggingContent = false

	override func resetCursorRects() {
		super.resetCursorRects()

		addCursorRect(bounds, cursor: canDrag ? .openHand : .arrow)
	}

	override func mouseDown(with event: NSEvent) {
		guard canDrag, let dragScrollView else {
			super.mouseDown(with: event)
			return
		}

		dragStartPoint = convert(event.locationInWindow, from: nil)
		dragStartOffset = dragScrollView.contentView.bounds.origin.x
		isDraggingContent = false
		NSCursor.openHand.set()
	}

	override func mouseDragged(with event: NSEvent) {
		guard
			let dragStartPoint,
			let dragScrollView,
			canDrag
		else {
			super.mouseDragged(with: event)
			return
		}

		let currentPoint = convert(event.locationInWindow, from: nil)
		let deltaX = currentPoint.x - dragStartPoint.x
		if abs(deltaX) > AccountRunStripLayout.dragActivationDistance {
			isDraggingContent = true
		}
		guard isDraggingContent else {
			return
		}

		NSCursor.closedHand.set()
		scroll(dragScrollView, to: dragStartOffset - deltaX)
	}

	override func mouseUp(with event: NSEvent) {
		guard canDrag else {
			super.mouseUp(with: event)
			return
		}

		if isDraggingContent == false {
			onClick?(convert(event.locationInWindow, from: nil))
		}

		dragStartPoint = nil
		isDraggingContent = false
		NSCursor.openHand.set()
	}

	private var canDrag: Bool {
		guard let dragScrollView else {
			return false
		}

		let contentWidth = dragScrollView.documentView?.frame.width ?? 0
		return contentWidth > dragScrollView.contentView.bounds.width + 1
	}

	private func scroll(_ scrollView: NSScrollView, to offset: CGFloat) {
		let clipView = scrollView.contentView
		let contentWidth = scrollView.documentView?.frame.width ?? 0
		let maxOffset = max(0, contentWidth - clipView.bounds.width)
		let clampedOffset = min(max(0, offset), maxOffset)

		clipView.scroll(to: NSPoint(x: clampedOffset, y: clipView.bounds.origin.y))
		scrollView.reflectScrolledClipView(clipView)
		onDragScroll?()
	}
}

private enum AccountPanelLayout {
	static let accountListScrollSpace = "account-list-scroll"
	static let screenVerticalMargin: CGFloat = 44
	static let panelVerticalPadding: CGFloat = 18
	static let sectionSpacing: CGFloat = 6
	static let headerHeight: CGFloat = 28
	static let accountSummaryHeight: CGFloat = 31
	static let telemetryHorizontalPadding: CGFloat = 7
	static let telemetryTopPadding: CGFloat = 7
	static let telemetryBottomPadding: CGFloat = 2
	static let telemetryVerticalPadding: CGFloat = telemetryTopPadding + telemetryBottomPadding
	static let telemetryRowSpacing: CGFloat = 5
	static let telemetryProfileHeight: CGFloat = 50
	static let telemetryPoolHeight: CGFloat = 16
	static let telemetryPoolMeasuredHeight: CGFloat = 29
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
	static let height: CGFloat = 18.5
	static let cornerRadius: CGFloat = 9.25
	static let horizontalPadding: CGFloat = 6.5
	static let iconWidth: CGFloat = 9.5
	static let spacing: CGFloat = 4
	static let popoverHoverDelayNanoseconds: UInt64 = 320_000_000
}

struct AccountRunChipView: View {
	let run: OperatorRunStatus
	let currentTime: Date
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false
	@State private var showsPopover = false
	@State private var hoverPopoverTask: Task<Void, Never>?

	var body: some View {
		HStack(spacing: AccountRunChipLayout.spacing) {
			Image(systemName: symbol)
				.font(PanelFont.runChipIcon)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.88 : 0.76))
				.frame(width: AccountRunChipLayout.iconWidth)

			Text(run.compactTitle)
				.font(PanelFont.runChipTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.92))
				.lineLimit(1)
				.truncationMode(.middle)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: AccountRunChipLayout.height)
		.padding(.horizontal, AccountRunChipLayout.horizontalPadding)
		.background {
			RoundedRectangle(cornerRadius: AccountRunChipLayout.cornerRadius, style: .continuous)
				.fill(isHovered ? tint.opacity(colorScheme == .dark ? 0.09 : 0.07) : Color.clear)
		}
		.modernGlassSurface(cornerRadius: AccountRunChipLayout.cornerRadius, depth: .row)
		.contentShape(RoundedRectangle(cornerRadius: AccountRunChipLayout.cornerRadius, style: .continuous))
		.onHover { hovering in
			isHovered = hovering
			if hovering {
				schedulePopover()
			} else {
				cancelPopover()
			}
		}
		.onDisappear {
			cancelPopover()
		}
		.popover(isPresented: $showsPopover, arrowEdge: .trailing) {
			OperatorLanePopoverView(run: run, currentTime: currentTime)
				.fixedSize(horizontal: true, vertical: false)
		}
	}

	private func schedulePopover() {
		hoverPopoverTask?.cancel()
		hoverPopoverTask = Task {
			try? await Task.sleep(nanoseconds: AccountRunChipLayout.popoverHoverDelayNanoseconds)
			guard Task.isCancelled == false else {
				return
			}

			await MainActor.run {
				if isHovered {
					showsPopover = true
				}
			}
		}
	}

	private func cancelPopover() {
		hoverPopoverTask?.cancel()
		hoverPopoverTask = nil
		showsPopover = false
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

private struct AccountProfileAggregate: Equatable {
	let accountCount: Int
	let lifetimeTokens: Int?
	let peakDailyTokens: Int?
	let longestTaskSeconds: Int?
	let currentStreakDays: Int?
	let longestStreakDays: Int?
	let dailyUsage: [AccountProfileDailyUsage]

	static func make(accounts: [CodexAccount]) -> AccountProfileAggregate? {
		var lifetimeTokens: Int?
		var peakFallbackTokens: Int?
		var longestTaskSeconds: Int?
		var currentStreakDays: Int?
		var longestStreakDays: Int?
		var usageByDate: [String: Int] = [:]

		for account in accounts {
			if let value = account.profileLifetimeTokens {
				lifetimeTokens = (lifetimeTokens ?? 0) + value
			}
			if let value = account.profilePeakDailyTokens {
				peakFallbackTokens = (peakFallbackTokens ?? 0) + value
			}
			if let value = account.profileLongestTaskSeconds {
				longestTaskSeconds = max(longestTaskSeconds ?? 0, value)
			}
			if let value = account.profileCurrentStreakDays {
				currentStreakDays = max(currentStreakDays ?? 0, value)
			}
			if let value = account.profileLongestStreakDays {
				longestStreakDays = max(longestStreakDays ?? 0, value)
			}
			for record in account.recentProfileDailyUsage {
				usageByDate[record.date, default: 0] += record.tokens
			}
		}

		let dailyUsage = usageByDate
			.map { AccountProfileDailyUsage(date: $0.key, tokens: $0.value) }
			.sorted { $0.date < $1.date }
		let peakDailyTokens = dailyUsage.map(\.tokens).max() ?? peakFallbackTokens
		let aggregate = AccountProfileAggregate(
			accountCount: accounts.count,
			lifetimeTokens: lifetimeTokens,
			peakDailyTokens: peakDailyTokens,
			longestTaskSeconds: longestTaskSeconds,
			currentStreakDays: currentStreakDays,
			longestStreakDays: longestStreakDays,
			dailyUsage: dailyUsage
		)

		return aggregate.hasProfileSummary ? aggregate : nil
	}

	var hasProfileSummary: Bool {
		lifetimeTokens != nil
			|| peakDailyTokens != nil
			|| longestTaskSeconds != nil
			|| currentStreakDays != nil
			|| longestStreakDays != nil
			|| dailyUsage.isEmpty == false
	}
}

private struct AccountTelemetryMatrixView: View {
	let aggregate: AccountProfileAggregate?
	let usageEstimate: AccountUsageEstimate?
	let accounts: [CodexAccount]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: AccountPanelLayout.telemetryRowSpacing) {
			if let aggregate {
				AccountProfileOverviewView(aggregate: aggregate)
			}

			if let usageEstimate {
				AccountPoolUsageEstimateView(estimate: usageEstimate, accounts: accounts)
			}
		}
		.padding(.horizontal, AccountPanelLayout.telemetryHorizontalPadding)
		.padding(.top, AccountPanelLayout.telemetryTopPadding)
		.padding(.bottom, AccountPanelLayout.telemetryBottomPadding)
		.frame(maxWidth: .infinity, alignment: .leading)
		.background {
			RoundedRectangle(cornerRadius: 9, style: .continuous)
				.fill(surfaceFill)
		}
		.clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
		.id(colorScheme == .dark ? "telemetry-matrix-dark" : "telemetry-matrix-light")
	}

	private var surfaceFill: Color {
		colorScheme == .dark
			? Color(red: 0.08, green: 0.095, blue: 0.13).opacity(0.34)
			: Color(red: 0.9, green: 0.94, blue: 0.98).opacity(0.48)
	}
}

private struct AccountProfileOverviewView: View {
	let aggregate: AccountProfileAggregate
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 5) {
			HStack(alignment: .firstTextBaseline, spacing: 5) {
				PanelMetricIconView(
					symbol: "sum",
					tint: PanelPalette.usageCyan(colorScheme).opacity(0.9)
				)

				Text("All accounts")
					.font(PanelFont.metricLabel)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)

				Spacer(minLength: 6)

				Text("\(aggregate.accountCount) accounts")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
					.lineLimit(1)
			}

			HStack(spacing: 5) {
				ForEach(Array(metrics.enumerated()), id: \.offset) { index, metric in
					HStack(alignment: .firstTextBaseline, spacing: 3) {
						Text(metric.label)
							.font(PanelFont.usageLabel)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
							.lineLimit(1)

						Text(metric.value)
							.font(PanelFont.usageValue)
							.foregroundStyle(index == 0 ? primaryMetricColor : PanelPalette.secondaryText(colorScheme))
							.monospacedDigit()
							.lineLimit(1)
							.minimumScaleFactor(0.72)
					}

					if index < metrics.count - 1 {
						Spacer(minLength: 3)
					}
				}
			}
			.frame(height: 16)

			if aggregate.dailyUsage.isEmpty == false {
				AccountProfileDailyUsageStripView(records: aggregate.dailyUsage)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityLabel)
	}

	private var metrics: [(label: String, value: String)] {
		[
			aggregate.lifetimeTokens.map { ("tok", formatCompactCount($0)) },
			aggregate.peakDailyTokens.map { ("peak", formatCompactCount($0)) },
			streakText.map { ("streak", $0) },
			aggregate.longestTaskSeconds
				.flatMap(formatActivityDuration)
				.map { ("task", $0) },
		]
		.compactMap { $0 }
	}

	private var streakText: String? {
		if let current = aggregate.currentStreakDays,
			let longest = aggregate.longestStreakDays
		{
			return "\(current)/\(longest)d"
		}
		if let current = aggregate.currentStreakDays {
			return "\(current)d"
		}
		if let longest = aggregate.longestStreakDays {
			return "\(longest)d"
		}

		return nil
	}

	private var primaryMetricColor: Color {
		PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.92 : 0.86)
	}

	private var accessibilityLabel: String {
		"All account profile totals, " + metrics.map { "\($0.label) \($0.value)" }.joined(separator: ", ")
	}
}

struct AccountPoolUsageEstimateView: View {
	let estimate: AccountUsageEstimate
	let accounts: [CodexAccount]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 3) {
			HStack(spacing: 5) {
				ForEach(Array(metrics.enumerated()), id: \.offset) { index, metric in
					AccountPoolUsageMetricView(
						title: metric.title,
						value: metric.value,
						tint: metric.tint
					)

					if index < metrics.count - 1 {
						Spacer(minLength: 3)
					}
				}
			}
			.frame(height: 16)

			if estimate.accountEstimateCount < estimate.accountCount {
				Text("\(estimate.accountEstimateCount)/\(estimate.accountCount) accounts measured")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
					.lineLimit(1)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityLabel)
	}

	private var metrics: [(title: String, value: String, tint: Color)] {
		[
			(
				"Pool used",
				formatUsagePercent(estimate.totalUsedOfCapacityPercent),
				poolUsageTint
			),
			("Day Δ", dayDeltaText, dayDeltaTint),
			(
				"Daily avg",
				formatDailyUsageRate(estimate.averageDailyPoolPercent),
				PanelPalette.secondaryText(colorScheme)
			),
		]
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
		HStack(alignment: .firstTextBaseline, spacing: 3) {
			Text(title)
				.font(PanelFont.usageLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
				.lineLimit(1)

			Text(value)
				.font(PanelFont.usageValue)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.94 : 0.78))
				.monospacedDigit()
				.lineLimit(1)
				.minimumScaleFactor(0.72)
		}
		.lineLimit(1)
	}
}

struct AccountUsageSummaryView: View {
	let account: CodexAccount
	let currentTime: Date

	var body: some View {
		VStack(spacing: 5) {
			if account.hasProfileSummary {
				AccountProfileSummaryView(account: account)
			}

			if account.hasPrimaryUsageData {
				AccountUsageMeterView(
					label: account.windowLabel(seconds: account.primaryWindowSeconds),
					remainingPercent: account.primaryRemainingPercent,
					resetAtUnixEpoch: account.primaryResetsAtUnixEpoch,
					dailyAveragePercent: account.sevenDayAveragePercent(
						forWindowSeconds: account.primaryWindowSeconds
					),
					tone: account.usageTone(remainingPercent: account.primaryRemainingPercent),
					currentTime: currentTime
				)
			}

			if account.hasSecondaryUsageData {
				AccountUsageMeterView(
					label: account.windowLabel(seconds: account.secondaryWindowSeconds),
					remainingPercent: account.secondaryRemainingPercent,
					resetAtUnixEpoch: account.secondaryResetsAtUnixEpoch,
					dailyAveragePercent: account.sevenDayAveragePercent(
						forWindowSeconds: account.secondaryWindowSeconds
					),
					tone: account.usageTone(remainingPercent: account.secondaryRemainingPercent),
					currentTime: currentTime
				)
			}
		}
		.frame(maxWidth: .infinity)
		.padding(.horizontal, 1)
		.padding(.vertical, 1)
	}
}

struct AccountProfileSummaryView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(spacing: 4) {
			if metrics.isEmpty == false {
				HStack(alignment: .firstTextBaseline, spacing: 5) {
					PanelMetricIconView(
						symbol: "chart.bar.xaxis",
						tint: PanelPalette.secondaryText(colorScheme).opacity(0.82)
					)

					ForEach(Array(metrics.enumerated()), id: \.offset) { index, metric in
						HStack(alignment: .firstTextBaseline, spacing: 3) {
							Text(metric.label)
								.font(PanelFont.usageLabel)
								.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
								.lineLimit(1)

							Text(metric.value)
								.font(PanelFont.usageValue)
								.foregroundStyle(valueColor(index: index))
								.monospacedDigit()
								.lineLimit(1)
						}

						if index < metrics.count - 1 {
							Spacer(minLength: 3)
						}
					}
				}
				.frame(height: 16)
			}

			if account.recentProfileDailyUsage.isEmpty == false {
				AccountProfileDailyUsageStripView(records: account.recentProfileDailyUsage)
			}
		}
		.accessibilityLabel(accessibilityLabel)
	}

	private var metrics: [(label: String, value: String)] {
		[
			account.profileLifetimeTokens.map { ("tok", formatCompactCount($0)) },
			account.profilePeakDailyTokensForDisplay.map { ("peak", formatCompactCount($0)) },
			streakText.map { ("streak", $0) },
			account.profileLongestTaskSeconds
				.flatMap(formatActivityDuration)
				.map { ("task", $0) },
		]
		.compactMap { $0 }
	}

	private var streakText: String? {
		if let current = account.profileCurrentStreakDays,
			let longest = account.profileLongestStreakDays
		{
			return "\(current)/\(longest)d"
		}
		if let current = account.profileCurrentStreakDays {
			return "\(current)d"
		}
		if let longest = account.profileLongestStreakDays {
			return "\(longest)d"
		}

		return nil
	}

	private var accessibilityLabel: String {
		metrics.map { "\($0.label) \($0.value)" }.joined(separator: ", ")
	}

	private func valueColor(index: Int) -> Color {
		index == 0
			? PanelPalette.primaryText(colorScheme).opacity(colorScheme == .dark ? 0.92 : 0.86)
			: PanelPalette.secondaryText(colorScheme)
	}
}

struct AccountProfileDailyUsageStripView: View {
	let records: [AccountProfileDailyUsage]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 2) {
			ForEach(Array(displayRecords.enumerated()), id: \.offset) { _, record in
				RoundedRectangle(cornerRadius: 2, style: .continuous)
					.fill(tileColor(tokens: record.tokens))
					.frame(width: 6, height: 9)
					.help("\(compactUsageDate(record.date)): \(formatCompactCount(record.tokens)) tokens")
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.frame(height: 11)
		.accessibilityHidden(true)
	}

	private var displayRecords: [AccountProfileDailyUsage] {
		Array(records.sorted { $0.date < $1.date }.suffix(36))
	}

	private var peakTokens: Int {
		max(1, displayRecords.map(\.tokens).max() ?? 1)
	}

	private func tileColor(tokens: Int) -> Color {
		let intensity = max(0.16, min(1, Double(tokens) / Double(peakTokens)))
		return PanelPalette.usageCyan(colorScheme).opacity(0.24 + 0.62 * intensity)
	}
}

struct AccountUsageMeterView: View {
	let label: String
	let remainingPercent: Int?
	let resetAtUnixEpoch: Int?
	let dailyAveragePercent: Double?
	let tone: AccountTone
	let currentTime: Date
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

				if let dailyAverageText {
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
							.minimumScaleFactor(0.78)
					}
					.layoutPriority(1)
				}

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
		.accessibilityLabel(accessibilityText)
	}

	private var remainingText: String {
		guard let remainingPercent else {
			return "-"
		}

		return "\(remainingPercent)% left"
	}

	private var dailyAverageText: String? {
		guard let dailyAveragePercent else {
			return nil
		}
		let formatted = formatDailyUsageRate(dailyAveragePercent)

		return formatted == "-" ? nil : formatted
	}

	private var accessibilityText: String {
		let average = dailyAverageText.map { ", daily average \($0)" } ?? ""
		return "\(label) remaining \(remainingText)\(average), \(resetDisplay.accessibility)"
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
		UsageResetDisplay.make(resetAtUnixEpoch: resetAtUnixEpoch, now: currentTime)
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

struct UsageResetDisplay {
	let short: String
	let date: String
	let accessibility: String

	static func make(resetAtUnixEpoch: Int?, now: Date = Date()) -> UsageResetDisplay {
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

		let distanceSeconds = Int(floor(resetAt.timeIntervalSince(now)))
		if distanceSeconds <= 0 {
			let date = formatResetDate(resetAt, now: now)
			return UsageResetDisplay(
				short: "0m",
				date: date,
				accessibility: "reset at \(date), reset due now"
			)
		}

		let short = formatResetDuration(distanceSeconds)
		let date = formatResetDate(resetAt, now: now)
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

	private static func formatResetDate(_ date: Date, now: Date) -> String {
		let formatter = DateFormatter()
		formatter.locale = Locale(identifier: "en_US_POSIX")
		let calendar = Calendar(identifier: .gregorian)
		formatter.dateFormat = calendar.component(.year, from: date) == calendar.component(.year, from: now)
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

struct OperatorLanePopoverView: View {
	let run: OperatorRunStatus
	let currentTime: Date

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			header

			if hasReadoutContent {
				OperatorLaneReadoutDivider()
			}

			if let modelBucket {
				OperatorLaneProgressReadoutRow(
					title: "Model",
					percent: bucketPercent(modelBucket),
					elapsed: formatActivityDuration(bucketWallSeconds(modelBucket)) ?? "0s",
					total: formatActivityDuration(totalWallSeconds) ?? "0s",
					barShare: bucketShare(modelBucket)
				)
				if detailBuckets.isEmpty == false || contextReadoutRows.isEmpty == false {
					OperatorLaneReadoutDivider()
				}
			}

			VStack(alignment: .leading, spacing: 3) {
				ForEach(detailBuckets) { bucket in
					OperatorLaneReadoutRow(title: rawPanelToken(bucket.name), items: bucketReadoutItems(bucket))
				}

				if contextReadoutRows.isEmpty == false {
					OperatorLaneReadoutDivider()
					ForEach(contextReadoutRows) { row in
						OperatorLaneReadoutRow(title: row.title, items: row.items)
					}
				}

				if detailBuckets.isEmpty, contextReadoutRows.isEmpty, fallbackRunReadoutItems.isEmpty == false {
					OperatorLaneReadoutRow(title: "Run", items: fallbackRunReadoutItems)
				}
			}
		}
		.padding(.horizontal, 10)
		.padding(.vertical, 7)
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityLabel("Lane activity for \(run.compactTitle)")
	}

	private var activity: OperatorChildAgentActivity? {
		run.childAgentActivity
	}

	private var currentSummary: String {
		if run.processAlive == false {
			if let idle = formatActivityDuration(run.inactiveDurationSeconds) {
				return "Stopped · idle \(idle)"
			}

			return "Stopped"
		}

		guard let activity else {
			return "Waiting for child activity"
		}

		let label = panelTrimmed(activity.currentDetail)
			?? panelTrimmed(activity.currentBucket).map(rawPanelToken)
			?? "Active"
		if let elapsed = formatActivityDuration(activity.currentElapsedSeconds(at: currentTime)) {
			return "\(rawPanelToken(label)) · \(elapsed)"
		}

		return rawPanelToken(label)
	}

	private var header: some View {
		OperatorLaneHeaderReadoutView(
			status: currentSummary,
			project: projectTitle
		)
	}

	private var projectTitle: String? {
		panelTrimmed(run.projectDisplayName) ?? panelTrimmed(run.projectID)
	}

	private var hasReadoutContent: Bool {
		modelBucket != nil
			|| detailBuckets.isEmpty == false
			|| contextReadoutRows.isEmpty == false
			|| fallbackRunReadoutItems.isEmpty == false
	}

	private var fallbackRunReadoutItems: [OperatorLaneReadoutItem] {
		guard let activity else {
			return []
		}

		var items = [
			OperatorLaneReadoutItem(
				label: "wall",
				value: formatActivityDuration(activity.wallSeconds(at: currentTime)) ?? "0s"
			),
			OperatorLaneReadoutItem(
				label: "events",
				value: formatCompactCount(activity.eventCount)
			),
			OperatorLaneReadoutItem(
				label: "input",
				value: "\(formatCompactCount(activity.inputTokensCumulative)) tok"
			),
			OperatorLaneReadoutItem(
				label: "output",
				value: "\(formatCompactCount(activity.outputTokensCumulative)) tok"
			),
			OperatorLaneReadoutItem(
				label: "tool calls",
				value: formatCompactCount(activity.toolCallCount)
			),
		]

		if let largestOutput = activity.largestToolOutputBytes, largestOutput > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "largest output",
					value: formatCompactBytes(largestOutput)
				)
			)
		}

		return items
	}

	private var modelBucket: OperatorChildAgentBucket? {
		return orderedBuckets.first { bucket in
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
			let leftWallSeconds = bucketWallSeconds(left)
			let rightWallSeconds = bucketWallSeconds(right)
			if leftWallSeconds != rightWallSeconds {
				return leftWallSeconds > rightWallSeconds
			}
			if left.eventCount != right.eventCount {
				return left.eventCount > right.eventCount
			}

			return left.name < right.name
		}
	}

	private var contextReadoutRows: [OperatorLaneReadoutLine] {
		let rows = [
			OperatorLaneReadoutLine(title: "Context", items: contextTokenReadoutItems),
			OperatorLaneReadoutLine(title: "Tools", items: contextToolReadoutItems),
		]

		return rows.filter { $0.items.isEmpty == false }
	}

	private var contextTokenReadoutItems: [OperatorLaneReadoutItem] {
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

		return items
	}

	private var contextToolReadoutItems: [OperatorLaneReadoutItem] {
		guard let activity else {
			return []
		}

		var items = [OperatorLaneReadoutItem]()
		if activity.toolCallCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "tool calls", value: formatCompactCount(activity.toolCallCount)))
		}
		if let largestOutput = activity.largestToolOutputBytes, largestOutput > 0 {
			items.append(OperatorLaneReadoutItem(label: "largest output", value: formatCompactBytes(largestOutput)))
		}
		if let largestTool = panelTrimmed(activity.largestToolOutputTool) {
			items.append(OperatorLaneReadoutItem(label: "largest tool", value: largestTool))
		}

		return items
	}

	private var bucketRows: [OperatorChildAgentBucket] {
		activity?.buckets ?? []
	}

	private var totalWallSeconds: Int {
		max(
			1,
			activity?.wallSeconds(at: currentTime) ?? 0,
			bucketRows.reduce(0) { $0 + max(0, bucketWallSeconds($1)) }
		)
	}

	private func bucketReadoutItems(_ bucket: OperatorChildAgentBucket) -> [OperatorLaneReadoutItem] {
		let normalizedName = bucket.name.lowercased()
		var items = [OperatorLaneReadoutItem]()

		let wallSeconds = bucketWallSeconds(bucket)

		if normalizedName.contains("tracker"), wallSeconds > 0 {
			items.append(OperatorLaneReadoutItem(label: "wall", value: formatActivityDuration(wallSeconds) ?? "0s"))
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
				items.append(OperatorLaneReadoutItem(label: "tool calls", value: formatCompactCount(bucket.toolCallCount)))
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
		let wallSeconds = bucketWallSeconds(bucket)

		guard wallSeconds > 0 else {
			return 0
		}

		return min(1, max(0.02, CGFloat(wallSeconds) / CGFloat(max(1, totalWallSeconds))))
	}

	private func bucketPercent(_ bucket: OperatorChildAgentBucket) -> Int {
		Int((Double(bucketWallSeconds(bucket)) / Double(max(1, totalWallSeconds)) * 100).rounded())
	}

	private func bucketWallSeconds(_ bucket: OperatorChildAgentBucket) -> Int {
		activity?.wallSeconds(for: bucket, at: currentTime) ?? bucket.wallSeconds
	}
}

struct OperatorLaneHeaderReadoutView: View {
	let status: String
	let project: String?
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 8) {
			Text(status)
				.font(OperatorLanePopoverStyle.titleFont)
				.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.tail)
				.fixedSize(horizontal: true, vertical: false)

			if let project = panelTrimmed(project) {
				Text(project)
					.font(OperatorLanePopoverStyle.projectFont)
					.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
					.lineLimit(1)
					.fixedSize(horizontal: true, vertical: false)
					.help(project)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}
}

struct OperatorLaneReadoutLine: Identifiable {
	let title: String
	let items: [OperatorLaneReadoutItem]

	var id: String {
		title
	}
}

private enum OperatorLaneReadoutLayout {
	static let titleWidth: CGFloat = 62
	static let columnSpacing: CGFloat = 7
	static let itemRowSpacing: CGFloat = 2
	static let progressTrackWidth: CGFloat = 84
}

private enum OperatorLanePopoverStyle {
	static let titleFont = PanelFont.laneTitle
	static let projectFont = PanelFont.laneDetail
	static let labelFont = PanelFont.usageLabel
	static let valueFont = PanelFont.lanePopoverMeta
	static let metaFont = PanelFont.tertiary
	static let separatorFont = PanelFont.tertiary

	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		Color.primary.opacity(colorScheme == .dark ? 0.82 : 0.76)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.76 : 0.7)
	}

	static func mutedText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.55 : 0.48)
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.13 : 0.18)
	}

	static func progressTrack(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.14 : 0.16)
	}

	static func progressFill(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.76 : 0.68)
	}
}

struct OperatorLaneReadoutItem: Identifiable {
	let label: String?
	let value: String

	init(label: String?, value: String) {
		self.label = label
		self.value = value
	}

	var id: String {
		"\(label ?? "value")-\(value)"
	}

	var displayValue: String {
		if value.hasSuffix(" tok") {
			return String(value.dropLast(4))
		}

		return value
	}

	fileprivate var summaryRuns: [OperatorLaneReadoutTextRun] {
		switch label?.lowercased() {
		case "wall":
			return [.meta("wall "), .value(displayValue)]
		case "events":
			return [.value(displayValue), .meta(" events")]
		case "input":
			return [.value(displayValue), .meta(" input")]
		case "output":
			return [.value(displayValue), .meta(" output")]
		case "current":
			return [.value(displayValue), .meta(" current")]
		case "peak":
			return [.value(displayValue), .meta(" peak")]
		case "tool calls":
			return [.value(displayValue), .meta(" calls")]
		case "output bytes":
			return [.value(displayValue), .meta(" output")]
		case "largest output":
			return [.value(displayValue), .meta(" max")]
		case "largest tool":
			return [.value(displayValue)]
		default:
			if let label {
				return [.meta("\(label) "), .value(displayValue)]
			}
			return [.value(displayValue)]
		}
	}

	func matchesLabel(_ expected: String) -> Bool {
		label?.caseInsensitiveCompare(expected) == .orderedSame
	}
}

fileprivate enum OperatorLaneReadoutTextRole {
	case meta
	case value
}

fileprivate struct OperatorLaneReadoutTextRun {
	let text: String
	let role: OperatorLaneReadoutTextRole

	static func meta(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .meta)
	}

	static func value(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .value)
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
		VStack(alignment: .leading, spacing: OperatorLaneReadoutLayout.itemRowSpacing) {
			HStack(alignment: .firstTextBaseline, spacing: OperatorLaneReadoutLayout.columnSpacing) {
				OperatorLaneReadoutLabelView(title: title)

				if summaryFragments.isEmpty == false {
					OperatorLaneReadoutSummaryView(fragments: summaryFragments)
						.lineLimit(1)
						.allowsTightening(true)
						.fixedSize(horizontal: true, vertical: false)
						.help(accessibilityText)
				} else {
					Spacer(minLength: 0)
				}

				if let trailing = panelTrimmed(trailing) {
					Text(trailing)
						.font(OperatorLanePopoverStyle.metaFont)
						.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
						.lineLimit(1)
						.fixedSize(horizontal: true, vertical: false)
				}
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}

	private var summaryFragments: [[OperatorLaneReadoutTextRun]] {
		let fragments = normalizedTitle == "tools"
			? toolSummaryFragments
			: items.map(\.summaryRuns)
		return fragments.filter { $0.isEmpty == false }
	}

	private var normalizedTitle: String {
		title.lowercased()
	}

	private var toolSummaryFragments: [[OperatorLaneReadoutTextRun]] {
		var fragments = [[OperatorLaneReadoutTextRun]]()
		if let calls = value(for: "tool calls") {
			fragments.append([.value(calls), .meta(" calls")])
		}
		if let maxOutput = value(for: "largest output") {
			if let largestTool = value(for: "largest tool") {
				fragments.append([.meta("max "), .value(maxOutput), .meta(" from "), .value(largestTool)])
			} else {
				fragments.append([.meta("max "), .value(maxOutput)])
			}
		} else if let largestTool = value(for: "largest tool") {
			fragments.append([.meta("largest "), .value(largestTool)])
		}

		return fragments
	}

	private func value(for label: String) -> String? {
		items.first { $0.matchesLabel(label) }?.displayValue
	}

	private var accessibilityText: String {
		items.map { item in
			if let label = item.label {
				return "\(label) \(item.value)"
			}
			return item.value
		}
		.joined(separator: ", ")
	}
}

fileprivate struct OperatorLaneReadoutSummaryView: View {
	let fragments: [[OperatorLaneReadoutTextRun]]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 0) {
			ForEach(fragments.indices, id: \.self) { index in
				OperatorLaneReadoutRunsView(runs: fragments[index])

				if index != fragments.indices.last {
					Text(" · ")
						.font(OperatorLanePopoverStyle.separatorFont)
						.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
				}
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}
}

fileprivate struct OperatorLaneReadoutRunsView: View {
	let runs: [OperatorLaneReadoutTextRun]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 0) {
			ForEach(runs.indices, id: \.self) { index in
				let run = runs[index]
				Text(run.text)
					.font(font(for: run.role))
					.foregroundStyle(foreground(for: run.role))
					.monospacedDigit()
					.lineLimit(1)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}

	private func font(for role: OperatorLaneReadoutTextRole) -> Font {
		switch role {
		case .meta:
			return OperatorLanePopoverStyle.metaFont
		case .value:
			return OperatorLanePopoverStyle.valueFont
		}
	}

	private func foreground(for role: OperatorLaneReadoutTextRole) -> Color {
		switch role {
		case .meta:
			return OperatorLanePopoverStyle.mutedText(colorScheme)
		case .value:
			return OperatorLanePopoverStyle.primaryText(colorScheme)
		}
	}
}

struct OperatorLaneReadoutLabelView: View {
	let title: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 3) {
			Image(systemName: symbol)
				.font(.system(size: 7.8, weight: .semibold))
				.foregroundStyle(tint)
				.frame(width: 9)

			Text(title)
				.font(OperatorLanePopoverStyle.labelFont)
				.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(width: OperatorLaneReadoutLayout.titleWidth, alignment: .leading)
	}

	private var symbol: String {
		switch title.lowercased() {
		case "model":
			return "waveform"
		case "protocol":
			return "network"
		case "tracker":
			return "clock"
		case "tool", "tools":
			return "hammer"
		case "context":
			return "text.alignleft"
		default:
			return "circle.fill"
		}
	}

	private var tint: Color {
		switch title.lowercased() {
		case "model":
			return PanelPalette.routeAccent(colorScheme).opacity(0.78)
		case "protocol":
			return PanelPalette.usageCyan(colorScheme).opacity(0.78)
		case "tracker":
			return PanelPalette.secondaryText(colorScheme).opacity(0.58)
		case "tool", "tools":
			return PanelPalette.codexAccent(colorScheme).opacity(0.72)
		case "context":
			return PanelPalette.capacityAccent(colorScheme).opacity(0.72)
		default:
			return PanelPalette.secondaryText(colorScheme).opacity(0.48)
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
		HStack(alignment: .center, spacing: OperatorLaneReadoutLayout.columnSpacing) {
			OperatorLaneReadoutLabelView(title: title)

			OperatorLanePopoverProgressBar(progress: barShare)
				.frame(width: OperatorLaneReadoutLayout.progressTrackWidth)

			OperatorLaneProgressTextView(percent: percent, elapsed: elapsed, total: total)
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: 16)
		.fixedSize(horizontal: true, vertical: false)
	}
}

struct OperatorLaneProgressTextView: View {
	let percent: Int
	let elapsed: String
	let total: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 0) {
			Text("\(percent)%")
				.font(OperatorLanePopoverStyle.valueFont)
				.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
				.monospacedDigit()

			Text(" · ")
				.font(OperatorLanePopoverStyle.separatorFont)
				.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))

			Text(elapsed)
				.font(OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
				.monospacedDigit()

			Text(" / ")
				.font(OperatorLanePopoverStyle.separatorFont)
				.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))

			Text(total)
				.font(OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
				.monospacedDigit()
		}
		.fixedSize(horizontal: true, vertical: false)
	}
}

struct OperatorLanePopoverProgressBar: View {
	let progress: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		GeometryReader { proxy in
			let width = max(0, min(1, progress)) * proxy.size.width
			ZStack(alignment: .leading) {
				Capsule()
					.fill(OperatorLanePopoverStyle.progressTrack(colorScheme))
				Capsule()
					.fill(OperatorLanePopoverStyle.progressFill(colorScheme))
					.frame(width: width)
			}
		}
		.frame(height: 3.5)
	}
}

struct OperatorLaneReadoutDivider: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Rectangle()
			.fill(OperatorLanePopoverStyle.separator(colorScheme))
			.frame(height: 0.5)
			.padding(.vertical, 0.5)
	}
}

private struct PanelMetricIconView: View {
	let symbol: String
	let tint: Color

	var body: some View {
		Image(systemName: symbol)
			.font(PanelFont.summaryIcon)
			.symbolRenderingMode(.monochrome)
			.foregroundStyle(tint)
			.frame(width: 12, height: 12)
			.alignmentGuide(.firstTextBaseline) { dimensions in
				dimensions[VerticalAlignment.center] + 3.85
			}
	}
}

struct SummaryTileView: View {
	let title: String
	let value: String
	let symbol: String
	let tint: Color
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 4) {
			PanelMetricIconView(
				symbol: symbol,
				tint: tint.opacity(colorScheme == .dark ? 0.78 : 0.82)
			)

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

enum AccountDisplay {
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

		return "\(local.prefix(3))...\(compactLocalSuffix(local))\(domain)"
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

	private static func compactLocalSuffix(_ local: String) -> String {
		if let separator = local.lastIndex(of: ".") {
			let segment = String(local[local.index(after: separator)...])
			if (2...4).contains(segment.count), segment.allSatisfy(\.isLetter) {
				return segment
			}
		}

		return String(local.suffix(3))
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

private func compactUsageDate(_ value: String) -> String {
	let formatter = DateFormatter()
	formatter.locale = Locale(identifier: "en_US_POSIX")
	formatter.dateFormat = "yyyy-MM-dd"
	guard let date = formatter.date(from: value) else {
		return value
	}

	formatter.dateFormat = "MMM d"
	return formatter.string(from: date)
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

private func panelTrimmed(_ value: String?) -> String? {
	value?.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func rawPanelToken(_ value: String) -> String {
	value.trimmingCharacters(in: .whitespacesAndNewlines)
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
		let appearanceID = colorScheme == .dark ? "dark" : "light"

		if #available(macOS 26.0, *) {
			content
				.background {
					shape.fill(surfaceFill)
				}
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
				// Menu-bar glass layers can keep a stale material across system appearance flips.
				// Re-key only the surface wrapper so light/dark changes redraw immediately.
				.id(appearanceID)
		} else {
			content
				.background {
					shape.fill(materialStyle)
					shape.fill(surfaceFill)
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
				.id(appearanceID)
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
				? Color(red: 0.08, green: 0.1, blue: 0.14).opacity(0.18)
				: Color.white.opacity(0.05)
		case .section:
			return colorScheme == .dark
				? Color(red: 0.13, green: 0.16, blue: 0.22).opacity(0.18)
				: Color.white.opacity(0.1)
		case .row:
			return colorScheme == .dark
				? Color(red: 0.11, green: 0.14, blue: 0.19).opacity(0.18)
				: Color.white.opacity(0.08)
		case .control:
			return colorScheme == .dark
				? Color(red: 0.16, green: 0.19, blue: 0.25).opacity(0.22)
				: Color.white.opacity(0.13)
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

	private var surfaceFill: Color {
		switch depth {
		case .panel:
			return colorScheme == .dark
				? Color(red: 0.04, green: 0.055, blue: 0.08).opacity(0.34)
				: Color(red: 0.95, green: 0.97, blue: 0.99).opacity(0.38)
		case .section:
			return colorScheme == .dark
				? Color(red: 0.12, green: 0.14, blue: 0.19).opacity(0.44)
				: Color(red: 0.8, green: 0.86, blue: 0.93).opacity(0.78)
		case .row:
			return colorScheme == .dark
				? Color(red: 0.095, green: 0.115, blue: 0.16).opacity(0.38)
				: Color(red: 0.82, green: 0.87, blue: 0.94).opacity(0.66)
		case .control:
			return colorScheme == .dark
				? Color(red: 0.12, green: 0.145, blue: 0.2).opacity(0.48)
				: Color(red: 0.74, green: 0.81, blue: 0.9).opacity(0.78)
		}
	}

	private var surfaceStroke: Color {
		switch depth {
		case .panel:
			return PanelPalette.glassStroke(colorScheme)
		case .section:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.94 : 0.86)
		case .row:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.72 : 0.66)
		case .control:
			return PanelPalette.glassStroke(colorScheme).opacity(colorScheme == .dark ? 0.68 : 0.64)
		}
	}

	private var strokeWidth: CGFloat {
		switch depth {
		case .panel:
			return 0.8
		case .section:
			return 0.7
		case .row, .control:
			return 0.6
		}
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
		hasUsageWindowSummary || hasProfileSummary
	}

	var hasUsageWindowSummary: Bool {
		hasPrimaryUsageData || hasSecondaryUsageData
	}

	var recentUsageRecords: [AccountUsageRecord] {
		usageRecords ?? []
	}

	func sevenDayAveragePercent(forWindowSeconds seconds: Int?) -> Double? {
		guard seconds == 604_800 else {
			return nil
		}

		return sevenDayDailyAveragePercent
	}

	var compactHealthLabel: String? {
		if isUsageLimited {
			return compactLimitStatusToken
		}

		if let token = recoveryAction?.trimmingCharacters(in: .whitespacesAndNewlines),
			token.isEmpty == false
		{
			return token
		}

		let label = status.trimmingCharacters(in: .whitespacesAndNewlines)
		return label.isEmpty || label == "available" ? nil : label
	}

	private var compactLimitStatusToken: String {
		let reached = rateLimitReachedType?.trimmingCharacters(in: .whitespacesAndNewlines)
		if let reached, reached.isEmpty == false, reached != "none" {
			return reached
		}

		let token = status.trimmingCharacters(in: .whitespacesAndNewlines)
		return token.isEmpty || token == "available" ? "usage_limited" : token
	}
}
