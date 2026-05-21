import AppKit
import Foundation
import SwiftUI

private enum PanelFont {
	private static func text(
		_ size: CGFloat,
		weight: Font.Weight,
		design: Font.Design = .default
	) -> Font {
		.system(size: size, weight: weight, design: design)
	}

	static let headerIcon = text(13.8, weight: .semibold)
	static let headerTitle = text(14.2, weight: .semibold)
	static let headerSubtitle = text(10.6, weight: .medium)
	static let emptyIcon = text(16.5, weight: .medium)
	static let emptyTitle = text(11.8, weight: .semibold)
	static let emptyBody = text(10.4, weight: .regular)
	static let notice = text(10.1, weight: .regular)
	static let summaryIcon = text(10, weight: .medium)
	static let summaryTitle = text(9.8, weight: .medium)
	static let summaryValue = text(11.3, weight: .semibold)
	static let accountName = text(12.6, weight: .semibold)
	static let accountDetail = text(10.4, weight: .medium)
	static let usage = text(9.6, weight: .semibold)
	static let usageMeta = text(8.9, weight: .medium)
	static let iconButton = text(11, weight: .semibold)
}

private enum PanelPalette {
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
			? Color(red: 0.86, green: 0.89, blue: 0.94)
			: Color(red: 0.22, green: 0.38, blue: 0.52)
	}

	static func routeAccent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.82, green: 0.87, blue: 0.94)
			: Color(red: 0.18, green: 0.34, blue: 0.52)
	}

	static func usageMint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.78, green: 0.84, blue: 0.9)
			: Color(red: 0.28, green: 0.38, blue: 0.5)
	}

	static func warning(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.88, green: 0.58, blue: 0.35)
			: Color(red: 0.62, green: 0.36, blue: 0.14)
	}

	static func destructive(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 1, green: 0.42, blue: 0.45)
			: Color(red: 0.72, green: 0.13, blue: 0.18)
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

private enum PanelMotion {
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
	@Environment(\.openWindow) private var openWindow
	@State private var pendingLogout: CodexAccount?
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
					}
				}
			),
			titleVisibility: .visible
		) {
			if let account = pendingLogout {
				Button("Log Out \(displayName(for: account))", role: .destructive) {
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
	}

	private var panelContent: some View {
		VStack(alignment: .leading, spacing: 6) {
			header
			accountSummary

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
		.frame(width: 310)
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
			}

			Spacer()

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
						presentLogin(.add)
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
				AccountRowView(
					account: account,
					emailsHidden: emailsHidden,
					showsDivider: index < store.accounts.count - 1,
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
					reauthenticate: {
						presentLogin(.reauth(displayName(for: account)))
					},
					logout: {
						pendingLogout = account
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
				let value = account(matching: selector)?.panelDisplayName(emailsHidden: true) ?? "Account"
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
			total + (account.hasUsageSummary ? 98 : 46)
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
		return hasFixedSelection ? "\(accountLabel) / run route set" : "\(accountLabel) / balanced runs"
	}

	private var emailsHidden: Bool {
		accountPrivacy != AccountPrivacy.visibleValue
	}

	private func displayName(for account: CodexAccount) -> String {
		account.panelDisplayName(emailsHidden: emailsHidden)
	}

	private func account(matching selector: String) -> CodexAccount? {
		store.accounts.first { account in
			account.matchesSelector(selector)
		}
	}

	private func presentLogin(_ mode: AccountLoginSheetMode) {
		loginWindowState.mode = mode
		store.resetLoginSession()
		NSApp.activate(ignoringOtherApps: true)
		openWindow(id: DecodexWindowID.login)
	}
}

struct AccountRowView: View {
	let account: CodexAccount
	let emailsHidden: Bool
	let showsDivider: Bool
	let useInCodex: () -> Void
	let routeRunsHere: () -> Void
	let reauthenticate: () -> Void
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
							action: reauthenticate,
							help: "Re-authenticate account"
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
						symbol: "trash",
						tint: PanelPalette.destructive(colorScheme),
						isActive: false,
						isDestructive: true,
						isSubtle: true,
						size: 21,
						action: logout,
						help: "Remove account"
					)
				}
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
	}

	private var displayName: String {
		account.panelDisplayName(emailsHidden: emailsHidden)
	}

	private var routeHelp: String {
		if account.selected {
			return "Restore balanced run routing"
		}
		if account.needsLogin {
			return "Re-authenticate before routing runs"
		}
		if account.disabled {
			return "Disabled account cannot route runs"
		}

		return "Route Decodex runs here"
	}
}

struct AccountUsageSummaryView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(spacing: 5) {
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
					.frame(width: 42, alignment: .leading)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))

				Text(remainingText)
					.frame(width: 40, alignment: .leading)
					.foregroundStyle(valueColor)
					.monospacedDigit()

				Spacer(minLength: 2)

				Text(resetDisplay.short)
					.font(PanelFont.usageMeta)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.82 : 0.9))
					.monospacedDigit()
					.lineLimit(1)

				if !resetDisplay.date.isEmpty {
					Text(resetDisplay.date)
						.font(PanelFont.usageMeta)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.68 : 0.78))
						.lineLimit(1)
						.truncationMode(.middle)
					}
			}
			.frame(height: 12)

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
		.font(PanelFont.usage)
		.lineLimit(1)
		.frame(height: 20)
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel("\(label) remaining \(remainingText), \(resetDisplay.accessibility)")
	}

	private var remainingText: String {
		guard let remainingPercent else {
			return "-"
		}

		return "\(remainingPercent)%"
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
		case .ready: return PanelPalette.usageMint(colorScheme)
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
			return color.opacity(colorScheme == .dark ? 0.92 : 0.72)
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
		formatter.locale = .autoupdatingCurrent
		let calendar = Calendar.autoupdatingCurrent
		let template = calendar.component(.year, from: date) == calendar.component(.year, from: Date())
			? "MMM d jm"
			: "MMM d y jm"
		formatter.setLocalizedDateFormatFromTemplate(template)
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
				.font(PanelFont.summaryTitle)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)

			Text(value)
				.font(PanelFont.summaryValue)
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

private enum GlassSurfaceDepth {
	case panel
	case section
	case row
	case control
}

private extension View {
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

private struct ModernGlassSurfaceModifier: ViewModifier {
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
		var glass = Glass.clear
		if depth == .control {
			glass = glass.interactive()
		}

		return glass
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
			return PanelPalette.usageMint(colorScheme)
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
		hasPrimaryUsageData || hasSecondaryUsageData
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
			return "Usage unknown"
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
