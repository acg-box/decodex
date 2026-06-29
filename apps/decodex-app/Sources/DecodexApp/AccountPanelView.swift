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

private struct OperatorLaneReadoutWidthKey: PreferenceKey {
	static let defaultValue: CGFloat = 0

	static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
		value = max(value, nextValue())
	}
}

private struct OperatorLaneReadoutWidthReader: View {
	var body: some View {
		GeometryReader { proxy in
			Color.clear
				.preference(key: OperatorLaneReadoutWidthKey.self, value: proxy.size.width)
		}
	}
}

struct OperatorLanePopoverView: View {
	let run: OperatorRunStatus
	let currentTime: Date
	@State private var readoutWidth: CGFloat = 0

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			VStack(alignment: .leading, spacing: 3) {
				if let projectTitle {
					measuredReadout {
						OperatorLaneReadoutRow(
							title: "Project",
							items: [OperatorLaneReadoutItem(label: "project", value: projectTitle)]
						)
					}
				}

				measuredReadout {
					OperatorLaneReadoutRow(
						title: "Activity",
						items: [OperatorLaneReadoutItem(label: nil, value: currentSummary)]
					)
				}

				if statusReadoutItems.isEmpty == false {
					measuredReadout {
						OperatorLaneReadoutRow(title: "Status", items: statusReadoutItems)
					}
				}

				if let modelProgress {
					measuredReadout {
						OperatorLaneProgressReadoutRow(
							title: modelProgress.title,
							percent: modelProgress.percent,
							elapsed: modelProgress.elapsed,
							total: modelProgress.total,
							barShare: modelProgress.barShare
						)
					}
				}

				if let continuationRecovery = run.continuationRecovery {
					measuredReadout {
						OperatorLaneRecoveryReadout(recovery: continuationRecovery)
					}
				}
			}

			if modelProgress != nil,
				totalOverviewMetrics.isEmpty == false
					|| detailBuckets.isEmpty == false
					|| lifecycleTableRows.isEmpty == false
			{
				alignedReadout {
					OperatorLaneReadoutDivider()
				}
			}

			VStack(alignment: .leading, spacing: 3) {
				if totalOverviewMetrics.isEmpty == false {
					measuredReadout {
						OperatorTotalMetricsView(metrics: totalOverviewMetrics)
					}
				}

				ForEach(detailBuckets) { bucket in
					measuredReadout {
						OperatorLaneReadoutRow(title: rawPanelToken(bucket.name), items: bucketReadoutItems(bucket))
					}
				}

				if lifecycleTableRows.isEmpty == false {
					if totalOverviewMetrics.isEmpty == false || detailBuckets.isEmpty == false {
						alignedReadout {
							OperatorLaneReadoutDivider()
						}
					}
					measuredReadout {
						OperatorLifecycleTableView(rows: lifecycleTableRows)
					}
				}

				if detailBuckets.isEmpty,
					totalOverviewMetrics.isEmpty,
					lifecycleTableRows.isEmpty,
					fallbackRunReadoutItems.isEmpty == false
				{
					measuredReadout {
						OperatorLaneReadoutRow(title: "Run", items: fallbackRunReadoutItems)
					}
				}
			}
		}
		.padding(.horizontal, 10)
		.padding(.vertical, 7)
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityLabel("Lane activity for \(run.compactTitle)")
		.onPreferenceChange(OperatorLaneReadoutWidthKey.self) { width in
			guard abs(width - readoutWidth) > 0.5 else {
				return
			}

			readoutWidth = width
		}
	}

	private var activity: OperatorChildAgentActivity? {
		run.childAgentActivity
	}

	private var currentSummary: String {
		if run.processAlive == false, run.hasFreshExecution == false {
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

	private var projectTitle: String? {
		panelTrimmed(run.projectDisplayName) ?? panelTrimmed(run.projectID)
	}

	private var alignedWidth: CGFloat? {
		readoutWidth > 0 ? readoutWidth : nil
	}

	private func measuredReadout<Content: View>(
		@ViewBuilder _ content: () -> Content
	) -> some View {
		content()
			.background(OperatorLaneReadoutWidthReader())
			.frame(width: alignedWidth, alignment: .leading)
	}

	private func alignedReadout<Content: View>(
		@ViewBuilder _ content: () -> Content
	) -> some View {
		content()
			.frame(width: alignedWidth, alignment: .leading)
	}

	private var hasReadoutContent: Bool {
		modelProgress != nil
			|| statusReadoutItems.isEmpty == false
			|| totalOverviewMetrics.isEmpty == false
			|| detailBuckets.isEmpty == false
			|| lifecycleTableRows.isEmpty == false
			|| fallbackRunReadoutItems.isEmpty == false
	}

	private var statusReadoutItems: [OperatorLaneReadoutItem] {
		var items = [OperatorLaneReadoutItem]()

		if let runPhase = panelTrimmed(run.runPhase ?? run.phase) {
			items.append(
				OperatorLaneReadoutItem(label: "run phase", value: rawPanelToken(runPhase))
			)
		}

		return items
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
				label: "tools",
				value: formatCompactCount(activity.toolCallCount)
			),
		]

		if let largestOutput = activity.largestToolOutputBytes, largestOutput > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "max output",
					value: formatLargestOutput(bytes: largestOutput)
				)
			)
		}

		return items
	}

	private var modelProgress: OperatorModelProgressReadout? {
		operatorModelProgressReadout(for: run, currentTime: currentTime)
	}

	private var detailBuckets: [OperatorChildAgentBucket] {
		guard run.lifecycleMetrics == nil else {
			return []
		}

		return orderedBuckets.filter { bucket in
			bucket.name.caseInsensitiveCompare("Model") != .orderedSame
				&& detailBucketIsVisible(bucket)
				&& bucketReadoutItems(bucket).isEmpty == false
		}
	}

	private func detailBucketIsVisible(_ bucket: OperatorChildAgentBucket) -> Bool {
		let normalizedName = bucket.name.lowercased()

		return normalizedName.contains("protocol") || normalizedName.contains("tracker")
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

	private var totalOverviewMetrics: [OperatorTotalMetric] {
		guard let lifecycleMetrics = run.lifecycleMetrics else {
			return []
		}

		return [
			contextMetric(lifecycleMetrics),
			trackerMetric(lifecycleMetrics),
			protocolMetric(lifecycleMetrics),
		].compactMap { $0 }
	}

	private func contextMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		var items = [OperatorLaneReadoutItem]()
		if metrics.inputTokensCumulative > 0 {
			items.append(OperatorLaneReadoutItem(label: "input", value: formatCompactCount(metrics.inputTokensCumulative)))
		}
		if metrics.outputTokensCumulative > 0 {
			items.append(OperatorLaneReadoutItem(label: "output", value: formatCompactCount(metrics.outputTokensCumulative)))
		}

		let current = metrics.inputTokensCurrent ?? activity?.inputTokensCurrent
		let peak = metrics.inputTokensPeak ?? activity?.inputTokensMax
		if let current {
			items.append(OperatorLaneReadoutItem(label: "current window", value: formatCompactCount(current)))
		}
		if let peak, peak != current {
			items.append(OperatorLaneReadoutItem(label: "peak window", value: formatCompactCount(peak)))
		}

		guard items.isEmpty == false else {
			return nil
		}

		return OperatorTotalMetric(
			title: "Context",
			items: items
		)
	}

	private func trackerMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		guard let bucket = lifecycleBucket(named: "Tracker", in: metrics.buckets) else {
			return nil
		}

		var items = [OperatorLaneReadoutItem]()
		if bucket.eventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(bucket.eventCount)))
		}
		if bucket.toolCallCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "tools", value: formatCompactCount(bucket.toolCallCount)))
		}
		if bucket.outputBytes > 0 {
			items.append(OperatorLaneReadoutItem(label: "output bytes", value: formatCompactBytes(bucket.outputBytes)))
		}

		guard items.isEmpty == false else {
			return nil
		}

		return OperatorTotalMetric(
			title: "Tracker",
			items: items
		)
	}

	private func protocolMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		guard metrics.protocolEventCount > 0 || metrics.childEventCount > 0 else {
			return nil
		}

		var items = [OperatorLaneReadoutItem]()
		if metrics.protocolEventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(metrics.protocolEventCount)))
		}
		if metrics.childEventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "child events", value: formatCompactCount(metrics.childEventCount)))
		}

		return OperatorTotalMetric(
			title: "Protocol",
			items: items
		)
	}

	private var lifecycleTableRows: [OperatorLifecycleTableRow] {
		guard let lifecycleMetrics = run.lifecycleMetrics else {
			return []
		}

		return lifecycleMetrics.phases.map { phase in
			lifecycleTableRow(
				lifecycleBucket: panelTrimmed(phase.label)
					?? panelTrimmed(phase.phase).map(rawPanelToken)
					?? "Lifecycle bucket",
				attemptCount: phase.attemptCount,
				wallSeconds: phase.wallSeconds,
				buckets: phase.buckets,
				inputTokens: phase.inputTokensCumulative,
				outputTokens: phase.outputTokensCumulative,
				toolCallCount: phase.toolCallCount,
				largestOutputBytes: phase.largestToolOutputBytes
			)
		}
	}

	private func lifecycleTableRow(
		lifecycleBucket: String,
		attemptCount: Int,
		wallSeconds: Int,
		buckets: [OperatorLifecycleMetricBucket],
		inputTokens: Int,
		outputTokens: Int,
		toolCallCount: Int,
		largestOutputBytes: Int?
	) -> OperatorLifecycleTableRow {
		let runtimeSeconds = lifecycleModelSeconds(buckets) ?? 0
		let runtime = runtimeShareParts(
			seconds: runtimeSeconds,
			totalSeconds: lifecycleWallSeconds(
				wallSeconds: wallSeconds,
				buckets: buckets,
				runtimeSeconds: runtimeSeconds
			)
		)
		let largestOutput = formatLargestOutput(bytes: largestOutputBytes)

		return OperatorLifecycleTableRow(
			lifecycleBucket: lifecycleBucket,
			attempts: attemptCount > 0 ? formatCompactCount(attemptCount) : "-",
			runtime: runtime.text,
			inputTokens: inputTokens > 0 ? formatCompactCount(inputTokens) : "-",
			outputTokens: outputTokens > 0 ? formatCompactCount(outputTokens) : "-",
			toolCalls: toolCallCount > 0 ? formatCompactCount(toolCallCount) : "-",
			largestOutput: largestOutput
		)
	}

	private func formatLargestOutput(bytes: Int?) -> String {
		guard let bytes, bytes > 0 else {
			return "-"
		}

		return formatCompactBytes(bytes)
	}

	private func lifecycleWallSeconds(
		wallSeconds: Int,
		buckets: [OperatorLifecycleMetricBucket],
		runtimeSeconds: Int
	) -> Int {
		max(
			1,
			wallSeconds,
			buckets.reduce(0) { $0 + max(0, $1.wallSeconds) },
			runtimeSeconds
		)
	}

	private func runtimeShareParts(
		seconds: Int,
		totalSeconds: Int
	) -> (percent: String, elapsed: String, total: String, ratio: String, text: String) {
		guard seconds > 0 else {
			return ("-", "-", "-", "-", "-")
		}

		let total = max(1, totalSeconds, seconds)
		let percent = Int((Double(seconds) / Double(total) * 100).rounded())
		let elapsed = formatActivityDuration(seconds) ?? "0s"
		let totalText = formatActivityDuration(total) ?? "0s"
		let compactElapsed = elapsed.replacingOccurrences(of: " ", with: "")
		let compactTotal = totalText.replacingOccurrences(of: " ", with: "")
		let ratio = "\(compactElapsed)/\(compactTotal)"
		return ("\(percent)%", elapsed, totalText, ratio, "\(ratio)(\(percent)%)")
	}

	private func lifecycleModelSeconds(_ buckets: [OperatorLifecycleMetricBucket]) -> Int? {
		buckets.first { bucket in
			bucket.name.caseInsensitiveCompare("Model") == .orderedSame
		}?.wallSeconds
	}

	private func lifecycleBucket(named name: String, in buckets: [OperatorLifecycleMetricBucket]) -> OperatorLifecycleMetricBucket? {
		buckets.first { bucket in
			bucket.name.caseInsensitiveCompare(name) == .orderedSame
		}
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
				items.append(OperatorLaneReadoutItem(label: "tools", value: formatCompactCount(bucket.toolCallCount)))
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

struct OperatorModelProgressReadout {
	let title: String
	let percent: Int
	let elapsed: String
	let total: String
	let barShare: CGFloat
}

fileprivate func operatorModelProgressReadout(
	for run: OperatorRunStatus,
	currentTime: Date
) -> OperatorModelProgressReadout? {
	if let lifecycleMetrics = run.lifecycleMetrics,
		let modelSeconds = operatorLifecycleModelSeconds(lifecycleMetrics.buckets)
	{
		let totalSeconds = max(
			1,
			lifecycleMetrics.wallSeconds,
			lifecycleMetrics.buckets.reduce(0) { $0 + max(0, $1.wallSeconds) },
			modelSeconds
		)
		let share = CGFloat(modelSeconds) / CGFloat(totalSeconds)

		return OperatorModelProgressReadout(
			title: "Inference",
			percent: Int((Double(modelSeconds) / Double(totalSeconds) * 100).rounded()),
			elapsed: formatActivityDuration(modelSeconds) ?? "0s",
			total: formatActivityDuration(totalSeconds) ?? "0s",
			barShare: min(1, max(0.02, share))
		)
	}

	guard let activity = run.childAgentActivity,
		let modelBucket = activity.buckets.first(where: { bucket in
			bucket.name.caseInsensitiveCompare("Model") == .orderedSame
		})
	else {
		return nil
	}

	let totalSeconds = max(
		1,
		activity.wallSeconds(at: currentTime),
		activity.buckets.reduce(0) { total, bucket in
			total + max(0, activity.wallSeconds(for: bucket, at: currentTime))
		}
	)
	let modelSeconds = activity.wallSeconds(for: modelBucket, at: currentTime)
	guard modelSeconds > 0 else {
		return nil
	}
	let share = CGFloat(modelSeconds) / CGFloat(totalSeconds)

	return OperatorModelProgressReadout(
		title: "Inference",
		percent: Int((Double(modelSeconds) / Double(totalSeconds) * 100).rounded()),
		elapsed: formatActivityDuration(modelSeconds) ?? "0s",
		total: formatActivityDuration(totalSeconds) ?? "0s",
		barShare: min(1, max(0.02, share))
	)
}

fileprivate func operatorLifecycleModelSeconds(
	_ buckets: [OperatorLifecycleMetricBucket]
) -> Int? {
	buckets.first { bucket in
		bucket.name.caseInsensitiveCompare("Model") == .orderedSame
	}?.wallSeconds
}

struct OperatorTotalMetric: Identifiable {
	let title: String
	let items: [OperatorLaneReadoutItem]

	var id: String {
		title
	}

	var accessibilityText: String {
		let itemText = items.map(\.accessibilityText).joined(separator: ", ")

		return itemText.isEmpty ? title : "\(title), \(itemText)"
	}
}

struct OperatorTotalMetricsView: View {
	let metrics: [OperatorTotalMetric]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Grid(
			alignment: .leading,
			horizontalSpacing: OperatorLaneReadoutLayout.totalValueLabelSpacing,
			verticalSpacing: OperatorLaneReadoutLayout.itemRowSpacing
		) {
			ForEach(metrics) { metric in
				GridRow(alignment: .firstTextBaseline) {
					OperatorLaneReadoutLabelView(title: metric.title)
						.gridColumnAlignment(.leading)
					ForEach(metricCells(for: metric)) { cell in
						metricCell(cell)
					}
				}
			}
		}
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityLabel(accessibilityText)
	}

	private var accessibilityText: String {
		metrics.map(\.accessibilityText).joined(separator: "; ")
	}

	private func metricCells(for metric: OperatorTotalMetric) -> [OperatorTotalMetricGridCell] {
		(0..<OperatorLaneReadoutLayout.metricColumnCount).flatMap { index in
			let item = metric.items.indices.contains(index) ? metric.items[index] : nil
			let placeholder = item == nil
			return [
				OperatorTotalMetricGridCell(
					id: "\(metric.id)-\(index)-value",
					slot: index,
					text: item?.displayValue ?? "-",
					accessibilityText: item?.accessibilityText,
					role: .value,
					isPlaceholder: placeholder
				),
				OperatorTotalMetricGridCell(
					id: "\(metric.id)-\(index)-label",
					slot: index,
					text: item?.label ?? "-",
					accessibilityText: item?.accessibilityText,
					role: .label,
					isPlaceholder: placeholder
				),
			]
		}
	}

	@ViewBuilder
	private func metricCell(_ cell: OperatorTotalMetricGridCell) -> some View {
		if cell.isPlaceholder {
			Color.clear
				.frame(width: 1, height: 1)
				.gridColumnAlignment(cell.role == .value ? .trailing : .leading)
				.accessibilityHidden(true)
		} else {
			Text(cell.text)
				.font(cell.role == .value ? OperatorLanePopoverStyle.valueFont : OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(cell.role == .value
					? OperatorLanePopoverStyle.primaryText(colorScheme)
					: OperatorLanePopoverStyle.mutedText(colorScheme))
				.monospacedDigit()
				.lineLimit(1)
				.allowsTightening(true)
				.padding(.leading, cell.role == .value && cell.slot > 0
					? OperatorLaneReadoutLayout.totalSegmentSpacing
					: 0)
				.gridColumnAlignment(cell.role == .value ? .trailing : .leading)
				.help(cell.accessibilityText ?? cell.text)
		}
	}
}

private struct OperatorTotalMetricGridCell: Identifiable {
	let id: String
	let slot: Int
	let text: String
	let accessibilityText: String?
	let role: OperatorTotalMetricGridCellRole
	let isPlaceholder: Bool
}

private enum OperatorTotalMetricGridCellRole {
	case value
	case label
}

struct OperatorLifecycleTableRow: Identifiable {
	let lifecycleBucket: String
	let attempts: String
	let runtime: String
	let inputTokens: String
	let outputTokens: String
	let toolCalls: String
	let largestOutput: String

	var id: String {
		lifecycleBucket
	}
}

struct OperatorLifecycleTableView: View {
	let rows: [OperatorLifecycleTableRow]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Grid(
			alignment: .leading,
			horizontalSpacing: OperatorLaneReadoutLayout.lifecycleTableColumnSpacing,
			verticalSpacing: OperatorLaneReadoutLayout.itemRowSpacing
		) {
			GridRow(alignment: .firstTextBaseline) {
				headerCell("Lifecycle bucket", alignment: .leading)
				headerCell("attempts", alignment: .trailing)
				headerCell("inference", alignment: .trailing)
				headerCell("input", alignment: .trailing)
				headerCell("output", alignment: .trailing)
				headerCell("tools", alignment: .trailing)
				headerCell("max output", alignment: .trailing)
			}
			ForEach(rows) { row in
				GridRow(alignment: .firstTextBaseline) {
					tableCell(row.lifecycleBucket, alignment: .leading)
					tableCell(row.attempts, alignment: .trailing)
					tableCell(row.runtime, alignment: .trailing)
					tableCell(row.inputTokens, alignment: .trailing)
					tableCell(row.outputTokens, alignment: .trailing)
					tableCell(row.toolCalls, alignment: .trailing)
					tableCell(row.largestOutput, alignment: .trailing)
				}
			}
		}
		.padding(.top, 2)
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityText)
	}

	private var accessibilityText: String {
		rows.map { row in
			"\(row.lifecycleBucket), attempts \(row.attempts), inference \(row.runtime), input \(row.inputTokens), output \(row.outputTokens), tools \(row.toolCalls), max output \(row.largestOutput)"
		}
		.joined(separator: "; ")
	}

	private func headerCell(
		_ text: String,
		alignment: HorizontalAlignment
	) -> some View {
		Text(text)
			.font(OperatorLanePopoverStyle.metaFont)
			.foregroundStyle(OperatorLanePopoverStyle.tableHeaderText(colorScheme))
			.lineLimit(1)
			.allowsTightening(true)
			.gridColumnAlignment(alignment)
	}

	private func tableCell(
		_ text: String,
		alignment: HorizontalAlignment
	) -> some View {
		Text(text)
			.font(OperatorLanePopoverStyle.metaFont)
			.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
			.monospacedDigit()
			.lineLimit(1)
			.allowsTightening(true)
			.gridColumnAlignment(alignment)
	}
}

private enum OperatorLaneReadoutLayout {
	static let titleWidth: CGFloat = 92
	static let columnSpacing: CGFloat = 7
	static let itemRowSpacing: CGFloat = 2
	static let progressTrackWidth: CGFloat = 84
	static let totalSegmentSpacing: CGFloat = 13
	static let totalValueLabelSpacing: CGFloat = 4
	static let metricColumnCount = 4
	static let lifecycleTableColumnSpacing: CGFloat = 18
}

private enum OperatorLanePopoverStyle {
	static let titleFont = PanelFont.laneTitle
	static let projectFont = PanelFont.laneDetail
	static let labelFont = PanelFont.lanePopoverLabel
	static let valueFont = PanelFont.lanePopoverValue
	static let metaFont = PanelFont.lanePopoverMeta
	static let separatorFont = PanelFont.lanePopoverMeta

	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		Color.primary.opacity(colorScheme == .dark ? 0.94 : 0.88)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.92 : 0.86)
	}

	static func mutedText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.78 : 0.68)
	}

	static func tableHeaderText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.72 : 0.62)
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.18 : 0.22)
	}

	static func progressTrack(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.2 : 0.22)
	}

	static func progressFill(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.86 : 0.76)
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

	var accessibilityText: String {
		if let label {
			return "\(label) \(displayValue)"
		}

		return displayValue
	}

	fileprivate var summaryRuns: [OperatorLaneReadoutTextRun] {
		switch label?.lowercased() {
		case "wall":
			return [.meta("wall "), .value(displayValue)]
		case "project":
			return [.meta(displayValue)]
		case "attempts":
			return [.value(displayValue), .meta(displayValue == "1" ? " attempt" : " attempts")]
		case "captured":
			return [.value(displayValue), .meta(" captured")]
		case "missing":
			return [.value(displayValue), .meta(" missing")]
		case "events":
			return [.value(displayValue), .meta(" events")]
		case "child events":
			return [.value(displayValue), .meta(" child events")]
		case "input", "input tokens":
			return [.value(displayValue), .meta(" input")]
		case "output", "output tokens":
			return [.value(displayValue), .meta(" output")]
		case "current":
			return [.value(displayValue), .meta(" current")]
		case "current window", "current window tokens":
			return [.value(displayValue), .meta(" current window")]
		case "peak":
			return [.value(displayValue), .meta(" peak")]
		case "peak window", "peak window tokens":
			return [.value(displayValue), .meta(" peak window")]
		case "tools", "tool calls":
			return [.value(displayValue), .meta(" tools")]
		case "output bytes":
			return [.value(displayValue), .meta(" output")]
		case "max output", "max tool output", "largest output":
			return [.value(displayValue), .meta(" max output")]
		case "largest tool", "source":
			return [.value(displayValue), .meta(" source")]
		default:
			if let label {
				return [.meta("\(label) "), .value(displayValue)]
			}
			return [.value(displayValue)]
		}
	}

	fileprivate var summaryCharacterCount: Int {
		summaryRuns.reduce(0) { total, run in
			total + run.text.count
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
		if let calls = value(for: "tools") ?? value(for: "tool calls") {
			fragments.append([.value(calls), .meta(" tools")])
		}
		if let maxOutput = value(for: "max output") ?? value(for: "max tool output") ?? value(for: "largest output") {
			fragments.append([.value(maxOutput), .meta(" max output")])
		}

		return fragments
	}

	private func value(for label: String) -> String? {
		items.first { $0.matchesLabel(label) }?.displayValue
	}

	private var accessibilityText: String {
		items.map(\.accessibilityText)
		.joined(separator: ", ")
	}
}

fileprivate struct OperatorLaneRecoveryReadout: View {
	let recovery: OperatorContinuationRecoveryStatus
	@State private var isExpanded = false
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: OperatorLaneReadoutLayout.itemRowSpacing) {
			Button {
				isExpanded.toggle()
			} label: {
				HStack(alignment: .firstTextBaseline, spacing: OperatorLaneReadoutLayout.columnSpacing) {
					OperatorLaneReadoutLabelView(title: "Recovery")

					OperatorLaneReadoutSummaryView(fragments: summaryFragments)
						.lineLimit(1)
						.allowsTightening(true)
						.fixedSize(horizontal: true, vertical: false)

					Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
						.font(.system(size: 7.5, weight: .semibold))
						.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
						.frame(width: 8, height: 8)
				}
				.contentShape(Rectangle())
			}
			.buttonStyle(.plain)
			.help(accessibilityText)

			if isExpanded {
				VStack(alignment: .leading, spacing: OperatorLaneReadoutLayout.itemRowSpacing) {
					ForEach(detailItems) { item in
						OperatorLaneRecoveryDetailRow(item: item)
					}
				}
				.padding(.leading, OperatorLaneReadoutLayout.titleWidth + OperatorLaneReadoutLayout.columnSpacing)
				.padding(.top, 1)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityElement(children: .combine)
		.accessibilityLabel(accessibilityText)
	}

	private var summaryFragments: [[OperatorLaneReadoutTextRun]] {
		var fragments = [[OperatorLaneReadoutTextRun]]()
		fragments.append([.value(token(recovery.state, fallback: "continuation"))])
		if recovery.budgetExceeded == true {
			fragments.append([.meta("budget "), .value("exceeded")])
		}

		return fragments
	}

	private var detailItems: [OperatorLaneReadoutItem] {
		[
			OperatorLaneReadoutItem(label: "state", value: token(recovery.state, fallback: "continuation")),
			OperatorLaneReadoutItem(label: "phase", value: phaseValue),
			OperatorLaneReadoutItem(label: "count", value: countValue),
			OperatorLaneReadoutItem(label: "error", value: token(recovery.sourceErrorClass, fallback: "unknown")),
		]
	}

	private var phaseValue: String {
		let phases = [
			tokenOrNil(recovery.sourcePhase),
			tokenOrNil(recovery.nextPhase),
		].compactMap { $0 }

		return phases.isEmpty ? "unknown" : phases.joined(separator: " -> ")
	}

	private var countValue: String {
		let count = recovery.recoveryCount ?? 0
		let limit = recovery.automaticContinuationLimit ?? 0
		let budget = recovery.budgetExceeded == true ? "exceeded" : "within"

		return "\(count)/\(limit) \(budget)"
	}

	private var accessibilityText: String {
		detailItems.map(\.accessibilityText)
			.joined(separator: ", ")
	}

	private func token(_ value: String?, fallback: String) -> String {
		tokenOrNil(value) ?? fallback
	}

	private func tokenOrNil(_ value: String?) -> String? {
		guard let value else {
			return nil
		}

		let token = rawPanelToken(value)

		return token.isEmpty ? nil : token
	}
}

fileprivate struct OperatorLaneRecoveryDetailRow: View {
	let item: OperatorLaneReadoutItem
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 6) {
			Text(item.label ?? "value")
				.font(OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
				.frame(width: 38, alignment: .leading)

			Text(item.displayValue)
				.font(OperatorLanePopoverStyle.valueFont)
				.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
				.monospacedDigit()
				.lineLimit(1)
				.truncationMode(.middle)
				.frame(maxWidth: 340, alignment: .leading)
				.help(item.accessibilityText)
		}
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
				.font(.system(size: 8.2, weight: .semibold))
				.foregroundStyle(tint)
				.frame(width: 9.5)

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
		case "model", "live model", "model time", "ai runtime", "inference":
			return "waveform"
		case "attempts":
			return "number"
		case "project":
			return "folder"
		case "activity":
			return "waveform.path.ecg"
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
		case "model", "live model", "model time", "ai runtime", "inference":
			return PanelPalette.routeAccent(colorScheme).opacity(0.78)
		case "attempts":
			return PanelPalette.routeAccent(colorScheme).opacity(0.68)
		case "project":
			return PanelPalette.landingAccent(colorScheme).opacity(0.74)
		case "activity":
			return PanelPalette.secondaryText(colorScheme).opacity(0.72)
		case "protocol":
			return PanelPalette.usageCyan(colorScheme).opacity(0.78)
		case "tracker":
			return PanelPalette.secondaryText(colorScheme).opacity(0.68)
		case "tool", "tools":
			return PanelPalette.codexAccent(colorScheme).opacity(0.72)
		case "context":
			return PanelPalette.capacityAccent(colorScheme).opacity(0.72)
		default:
			return PanelPalette.secondaryText(colorScheme).opacity(0.58)
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
				.frame(minWidth: OperatorLaneReadoutLayout.progressTrackWidth, maxWidth: .infinity)

			OperatorLaneProgressTextView(percent: percent, elapsed: elapsed, total: total)
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: 16)
		.frame(maxWidth: .infinity, alignment: .leading)
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
