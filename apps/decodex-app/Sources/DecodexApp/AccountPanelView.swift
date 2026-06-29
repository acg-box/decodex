import AppKit
import Combine
import Foundation
import SwiftUI

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
				let runs = operatorCurrentLaneCards(for: account)

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

	private var operatorCurrentLaneCards: [OperatorCurrentLaneCard] {
		store.operatorPresentation?.currentLaneCards
			?? store.operatorSnapshot?.presentation?.currentLaneCards
			?? []
	}

	private func operatorCurrentLaneCards(for account: CodexAccount) -> [OperatorCurrentLaneCard] {
		operatorCurrentLaneCards.filter { $0.isAssigned(to: account) }
	}

	private func accountRowHeight(for account: CodexAccount) -> CGFloat {
		let base: CGFloat
		if account.hasUsageWindowSummary {
			base = 102
		} else {
			base = 48
		}
		let runSignal: CGFloat = operatorCurrentLaneCards(for: account).isEmpty ? 0 : 22
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
	let runs: [OperatorCurrentLaneCard]
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

enum AccountPanelLayout {
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
