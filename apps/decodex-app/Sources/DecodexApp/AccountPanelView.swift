import AppKit
import Foundation
import SwiftUI

enum PanelFont {
	private static func text(
		_ size: CGFloat,
		weight: Font.Weight,
		design: Font.Design = .default
	) -> Font {
		.system(size: size, weight: weight, design: design)
	}

	static let headerIcon = text(14.1, weight: .semibold)
	static let headerTitle = text(14.8, weight: .semibold)
	static let headerSubtitle = text(11.1, weight: .medium)
	static let emptyIcon = text(16.8, weight: .medium)
	static let emptyTitle = text(12.2, weight: .semibold)
	static let emptyBody = text(10.9, weight: .regular)
	static let notice = text(10.6, weight: .regular)
	static let summaryIcon = text(10.4, weight: .medium)
	static let metricLabel = text(10.4, weight: .medium)
	static let metricValue = text(11.9, weight: .semibold)
	static let accountName = text(13.1, weight: .semibold)
	static let accountDetail = text(10.9, weight: .medium)
	static let usageLabel = text(10.4, weight: .medium)
	static let usageValue = text(10.7, weight: .semibold)
	static let tertiary = text(9.7, weight: .medium)
	static let iconButton = text(11.2, weight: .semibold)
}

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
		let responds = !isDisabled
		let hoverActive = responds && isHovered && !isPressed
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
					if !visible {
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
					updatedAt: store.operatorSnapshotUpdatedAt,
					refreshIntervalSeconds: AccountStore.operatorSnapshotRefreshIntervalSeconds
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
							await store.setFastMode(!store.fastModeEnabled)
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
			if store.accounts.count <= 3 {
				accountRows
			} else {
				ScrollView {
					accountRows
				}
				.frame(height: accountListHeight)
				.scrollIndicators(.hidden)
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
					runCount: operatorRunCount(for: account),
					emailsHidden: emailsHidden,
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
				return account.panelDisplayName(emailsHidden: true)
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

		if let selector = control.accountSelector, !selector.isEmpty {
			if emailsHidden {
				let value = account(matching: selector)?.panelDisplayName(emailsHidden: true)
					?? AccountDisplay.alias(forIdentity: selector)
				return "To \(value)"
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

		return !selector.isEmpty
	}

	private var accountListHeight: CGFloat {
		let rows = store.accounts.reduce(CGFloat(0)) { total, account in
			total + accountRowHeight(for: account)
		}
		let spacing = CGFloat(max(store.accounts.count - 1, 0)) * 5 + 2

		return min(
			rows + spacing,
			312
		)
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
		account.panelDisplayName(emailsHidden: emailsHidden)
	}

	private func operatorRuns(for account: CodexAccount) -> [OperatorRunStatus] {
		store.operatorSnapshot?.activeRuns(for: account) ?? []
	}

	private func operatorRunCount(for account: CodexAccount) -> Int? {
		guard let count = store.operatorSnapshot?.runningCount(for: account), count > 0 else {
			return nil
		}

		return count
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
	let runCount: Int?
	let emailsHidden: Bool
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

					if let planLabel = account.planLabel {
						Text("·")
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.62))
							.fixedSize(horizontal: true, vertical: false)

						Text(planLabel)
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

					if let runCount {
						Text("·")
							.font(PanelFont.accountDetail)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.62))
							.fixedSize(horizontal: true, vertical: false)

						Text(runCount == 1 ? "1 running" : "\(runCount) running")
							.font(PanelFont.accountDetail)
							.foregroundStyle(
								runCount > 0
									? PanelPalette.routeAccent(colorScheme)
									: PanelPalette.secondaryText(colorScheme).opacity(0.84)
							)
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
							isDisabled: account.codexActive || !account.canUseInCodex,
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
						isDisabled: !account.canRouteRuns && !account.selected,
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
						isSubtle: !isLogoutArmed,
						size: 21,
						action: logout,
						help: isLogoutArmed ? "Click again to confirm removal" : "Remove account"
					)
					.modifier(DeleteArmedShakeModifier(isArmed: isLogoutArmed))
				}
			}

			if !runs.isEmpty {
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

	private var displayName: String {
		account.panelDisplayName(emailsHidden: emailsHidden)
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
		HStack(spacing: 5) {
			ForEach(Array(runs.prefix(2))) { run in
				AccountRunChipView(run: run)
			}

			if runs.count > 2 {
				Text("+\(runs.count - 2)")
					.font(PanelFont.metricLabel)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.frame(height: 19)
					.padding(.horizontal, 6)
					.modernGlassSurface(cornerRadius: 9, depth: .control)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

struct AccountRunChipView: View {
	let run: OperatorRunStatus
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 4) {
			Image(systemName: symbol)
				.font(PanelFont.summaryIcon)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.9 : 0.78))
				.frame(width: 10)

			Text(run.compactTitle)
				.font(PanelFont.metricLabel)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.92))
				.lineLimit(1)
				.truncationMode(.middle)

			Text(run.compactDetail)
				.font(PanelFont.metricLabel)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.tail)
		}
		.frame(height: 19)
		.frame(maxWidth: 132, alignment: .leading)
		.padding(.horizontal, 6)
		.modernGlassSurface(cornerRadius: 9, depth: .control)
		.layoutPriority(1)
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
		guard !measuredAccounts.isEmpty, estimate.totalCapacityPercent > 0 else {
			return nil
		}

		let latestDate = measuredAccounts
			.flatMap(\.recentUsageRecords)
			.map(\.date)
			.max()
		guard let latestDate else {
			return estimate.totalUsedOfCapacityPercent
		}
		guard let previousDate = previousUsageDate(before: latestDate) else {
			return estimate.totalUsedOfCapacityPercent
		}

		let previousUsedPercent = measuredAccounts.reduce(0) { total, account in
			total + (usageRecord(for: account, on: previousDate)?.usedPercent ?? 0)
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

				if !resetDisplay.date.isEmpty {
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

	private var trackColor: Color {
		PanelPalette.progressTrack(colorScheme)
	}

	private var trackEdgeColor: Color {
		PanelPalette.progressEdge(colorScheme)
	}

	private var resetDisplay: UsageResetDisplay {
		UsageResetDisplay.make(resetAtUnixEpoch: resetAtUnixEpoch)
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
	let refreshIntervalSeconds: Int
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			if !metrics.isEmpty {
				HStack(spacing: 0) {
					ForEach(Array(metrics.enumerated()), id: \.element.id) { index, metric in
						if index > 0 {
							flowDivider
						}

						OperatorFlowMetricView(metric: metric)
					}
				}
				.frame(height: 32)
			}

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
				.help("Operator snapshot refreshes every \(refreshIntervalSeconds) seconds.")
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
		].filter { metric in
			metric.value > 0
		}
	}

	private var refreshMeta: String {
		guard let updatedAt else {
			return "\(refreshIntervalSeconds)s refresh"
		}

		let age = max(0, Int(Date().timeIntervalSince(updatedAt).rounded()))
		if age < 2 {
			return "\(refreshIntervalSeconds)s refresh"
		}

		return "\(age)s ago"
	}
}

struct OperatorFlowMetric: Identifiable {
	let title: String
	let value: Int
	let unitSingular: String
	let unitPlural: String
	let tint: Color

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
				.foregroundStyle(metric.tint)
				.monospacedDigit()
				.lineLimit(1)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(.horizontal, 5)
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
		.opacity(isDisabled && !isActive ? 0.56 : 1)
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
		alias(forIdentity: account.randomNameSeed)
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
	components.year = 2000
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
		} else {
			content
				.background {
					shape.fill(materialStyle)
				}
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

}

private extension CodexAccount {
	var randomNameSeed: String {
		if !accountFingerprint.isEmpty {
			return accountFingerprint
		}
		if let email, !email.isEmpty {
			return email
		}
		if let planType, !planType.isEmpty {
			return planType
		}

		return "account"
	}

	func panelDisplayName(emailsHidden: Bool) -> String {
		if emailsHidden {
			if let randomName = randomName?.trimmingCharacters(in: .whitespacesAndNewlines),
				!randomName.isEmpty
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
