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

	static let headerIcon = text(13.6, weight: .semibold)
	static let headerTitle = text(13.6, weight: .semibold)
	static let headerSubtitle = text(10.1, weight: .medium)
	static let emptyIcon = text(16.5, weight: .medium)
	static let emptyTitle = text(11.3, weight: .semibold)
	static let emptyBody = text(10.1, weight: .regular)
	static let notice = text(9.9, weight: .regular)
	static let summaryIcon = text(9.7, weight: .medium)
	static let summaryTitle = text(9.4, weight: .medium)
	static let summaryValue = text(10.9, weight: .semibold)
	static let accountName = text(11.9, weight: .semibold)
	static let accountDetail = text(9.9, weight: .medium)
	static let usage = text(8.45, weight: .medium)
	static let usageMeta = text(7.85, weight: .medium)
	static let primaryButton = text(10.7, weight: .semibold)
	static let iconButton = text(10.7, weight: .semibold)
	static let footerIcon = text(10.1, weight: .medium)
}

private enum PanelPalette {
	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.88, green: 0.94, blue: 1).opacity(0.96)
			: Color(red: 0.11, green: 0.17, blue: 0.26).opacity(0.94)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.6, green: 0.7, blue: 0.82).opacity(0.82)
			: Color(red: 0.3, green: 0.4, blue: 0.53).opacity(0.8)
	}

	static func panelTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.025, green: 0.075, blue: 0.13).opacity(0.68)
			: Color(red: 0.46, green: 0.68, blue: 0.9).opacity(0.62)
	}

	static func summaryTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.16, green: 0.28, blue: 0.42).opacity(0.22)
			: Color.white.opacity(0.58)
	}

	static func accountRowTint(_ colorScheme: ColorScheme, isSelected: Bool, isCodexActive: Bool) -> Color {
		if isSelected {
			return colorScheme == .dark
				? Color(red: 0.16, green: 0.32, blue: 0.5).opacity(0.34)
				: Color(red: 0.88, green: 0.96, blue: 1).opacity(0.76)
		}
		if isCodexActive {
			return colorScheme == .dark
				? Color(red: 0.13, green: 0.25, blue: 0.38).opacity(0.28)
				: Color.white.opacity(0.62)
		}

		return colorScheme == .dark
			? Color(red: 0.11, green: 0.2, blue: 0.32).opacity(0.24)
			: Color.white.opacity(0.6)
	}

	static func usageTray(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.015, green: 0.045, blue: 0.08).opacity(0.54)
			: Color.white.opacity(0.18)
	}

	static func usageTrayStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.52, green: 0.72, blue: 0.94).opacity(0.16)
			: Color.white.opacity(0.18)
	}

	static func addButtonTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.13, green: 0.31, blue: 0.5).opacity(0.48)
			: Color(red: 0.86, green: 0.94, blue: 1).opacity(0.84)
	}

	static func addButtonStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.5, green: 0.76, blue: 1).opacity(0.36)
			: Color(red: 0.24, green: 0.43, blue: 0.64).opacity(0.42)
	}

	static func controlTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.16, green: 0.3, blue: 0.46).opacity(0.34)
			: Color(red: 0.9, green: 0.96, blue: 1).opacity(0.86)
	}

	static func controlStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.55, green: 0.76, blue: 1).opacity(0.28)
			: Color(red: 0.28, green: 0.46, blue: 0.66).opacity(0.38)
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.065)
			: Color(red: 0.42, green: 0.58, blue: 0.75).opacity(0.18)
	}

	static func actionBlue(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.43, green: 0.74, blue: 1)
			: Color(red: 0.1, green: 0.28, blue: 0.46)
	}

	static func activeGold(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.95, green: 0.8, blue: 0.34)
			: Color(red: 0.28, green: 0.42, blue: 0.55)
	}

	static func usageMint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.36, green: 0.94, blue: 0.76)
			: Color(red: 0.38, green: 0.56, blue: 0.64)
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
			? Color(red: 0, green: 0.025, blue: 0.055).opacity(0.62)
			: Color(red: 0.12, green: 0.28, blue: 0.4).opacity(0.16)
	}

	static func progressEdge(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.56, green: 0.78, blue: 1).opacity(0.2)
			: Color.white.opacity(0.24)
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
		colorScheme == .dark ? -0.018 : -0.01
	}

	private var hoverShadowColor: Color {
		colorScheme == .dark
			? Color.black.opacity(0.18)
			: Color(red: 0.1, green: 0.24, blue: 0.38).opacity(0.12)
	}
}

struct AccountPanelView: View {
	@ObservedObject var store: AccountStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var pendingLogout: CodexAccount?
	@State private var loginPresented = false
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
		.sheet(isPresented: $loginPresented) {
			LoginSheetView(store: store)
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

			footer
		}
		.frame(width: 310)
		.padding(9)
		.modernGlassSurface(
			cornerRadius: 18,
			tint: PanelPalette.panelTint(colorScheme),
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
				.frame(width: 16, height: 16)
				.frame(width: 28, height: 28)
				.modernGlassSurface(
					cornerRadius: 8,
					tint: PanelPalette.controlTint(colorScheme),
					depth: .control
				)

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

			HStack(spacing: 4) {
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
					symbol: store.isRefreshing ? "arrow.triangle.2.circlepath.circle" : "arrow.clockwise",
					tint: PanelPalette.secondaryText(colorScheme),
					isActive: store.isRefreshing,
					isDisabled: store.isRefreshing,
					action: {
						Task {
							await store.refresh(force: true)
						}
					},
					help: "Refresh"
				)
			}
		}
	}

	private var accountSummary: some View {
		HStack(spacing: 0) {
			SummaryTileView(
				title: "Codex",
				value: codexAuthLabel,
				symbol: "person.crop.circle",
				tint: PanelPalette.activeGold(colorScheme)
			)

			Rectangle()
				.fill(PanelPalette.separator(colorScheme))
				.frame(width: 0.5)
				.padding(.vertical, 3)

			SummaryTileView(
				title: "Runs",
				value: decodexModeLabel,
				symbol: "arrow.triangle.branch",
				tint: hasFixedSelection ? PanelPalette.actionBlue(colorScheme) : PanelPalette.secondaryText(colorScheme)
			)
		}
		.padding(.horizontal, 7)
		.padding(.vertical, 4)
		.modernGlassSurface(
			cornerRadius: 9,
			tint: PanelPalette.summaryTint(colorScheme),
			depth: .row
		)
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
		VStack(spacing: 5) {
			ForEach(store.accounts) { account in
				AccountRowView(
					account: account,
					emailsHidden: emailsHidden,
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
					logout: {
						pendingLogout = account
					}
				)
			}
		}
		.padding(.vertical, 1)
	}

	private var footer: some View {
		HStack(spacing: 5) {
			PanelPrimaryButtonView(
				title: "Add Login",
				symbol: "plus.circle",
				action: {
					loginPresented = true
				}
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

		}
		.animation(PanelMotion.state, value: hasFixedSelection)
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
			total + (account.hasUsageSummary ? 82 : 44)
		}
		let spacing = CGFloat(max(store.accounts.count - 1, 0)) * 5 + 2

		return min(
			rows + spacing,
			278
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
}

struct AccountRowView: View {
	let account: CodexAccount
	let emailsHidden: Bool
	let useInCodex: () -> Void
	let routeRunsHere: () -> Void
	let logout: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			HStack(alignment: .top, spacing: 8) {
				VStack(alignment: .leading, spacing: 2) {
					Text(displayName)
						.font(PanelFont.accountName)
						.foregroundStyle(PanelPalette.primaryText(colorScheme))
						.lineLimit(1)
						.truncationMode(.middle)
						.layoutPriority(1)

					if account.hasVisibleMetadata {
						HStack(spacing: 4) {
							if let planLabel = account.planLabel {
								Text(planLabel)
									.lineLimit(1)
							}
							if account.planLabel != nil, account.compactHealthLabel != nil {
								Text("·")
							}
							if let healthLabel = account.compactHealthLabel {
								Text(healthLabel)
									.foregroundStyle(account.statusDisplayColor(colorScheme: colorScheme))
									.lineLimit(1)
							}
						}
						.font(PanelFont.accountDetail)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					}
				}
				.frame(maxWidth: .infinity, alignment: .leading)

				HStack(spacing: 3) {
					PanelIconButtonView(
						symbol: account.codexActive ? "person.crop.circle.fill" : "person.crop.circle",
						tint: PanelPalette.activeGold(colorScheme),
						isActive: account.codexActive,
						isDisabled: account.codexActive,
						isSubtle: true,
						size: 23,
						action: useInCodex,
						help: account.codexActive ? "Current Codex account" : "Use as Codex account"
					)

					PanelIconButtonView(
						symbol: "arrow.triangle.branch",
						tint: PanelPalette.actionBlue(colorScheme),
						isActive: account.selected,
						isSubtle: true,
						size: 23,
						action: routeRunsHere,
						help: account.selected ? "Restore balanced run routing" : "Route Decodex runs here"
					)

					PanelIconButtonView(
						symbol: "trash",
						tint: PanelPalette.destructive(colorScheme),
						isActive: false,
						isDestructive: true,
						size: 23,
						action: logout,
						help: "Remove account"
					)
				}
			}

			if account.hasUsageSummary {
				AccountUsageSummaryView(account: account)
			}
		}
		.padding(.vertical, 5)
		.padding(.leading, 8)
		.padding(.trailing, 7)
		.modernGlassSurface(
			cornerRadius: 9,
			tint: PanelPalette.accountRowTint(
				colorScheme,
				isSelected: account.selected,
				isCodexActive: account.codexActive
			),
			depth: .row
		)
		.panelInteractiveSurface(
			hoverLift: 0.35,
			hoverScale: 1.002,
			pressedScale: 1,
			hoverShadowRadius: 2
		)
		.animation(PanelMotion.state, value: account.selected)
		.animation(PanelMotion.state, value: account.codexActive)
	}

	private var displayName: String {
		account.panelDisplayName(emailsHidden: emailsHidden)
	}

}

struct AccountUsageSummaryView: View {
	let account: CodexAccount
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(spacing: 3) {
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
		.padding(.horizontal, 5)
		.padding(.vertical, 3)
		.background {
			RoundedRectangle(cornerRadius: 7, style: .continuous)
				.fill(PanelPalette.usageTray(colorScheme))
		}
		.overlay {
			RoundedRectangle(cornerRadius: 7, style: .continuous)
				.strokeBorder(PanelPalette.usageTrayStroke(colorScheme), lineWidth: 0.35)
				.allowsHitTesting(false)
		}
		.shadow(
			color: usageTrayShadow,
			radius: colorScheme == .dark ? 4.5 : 3.5,
			x: 0,
			y: colorScheme == .dark ? 1.4 : 1
		)
	}

	private var usageTrayShadow: Color {
		colorScheme == .dark
			? Color(red: 0, green: 0.05, blue: 0.11).opacity(0.26)
			: Color(red: 0.12, green: 0.28, blue: 0.42).opacity(0.055)
	}
}

struct AccountUsageMeterView: View {
	let label: String
	let remainingPercent: Int?
	let resetAtUnixEpoch: Int?
	let tone: AccountTone
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 2) {
			HStack(spacing: 5) {
				Text(label)
					.frame(width: 37, alignment: .leading)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))

				Text(remainingText)
					.frame(width: 32, alignment: .leading)
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
			.frame(height: 10)

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
						.animation(PanelMotion.state, value: remainingPercent)
						.shadow(
							color: color.opacity(colorScheme == .dark ? 0.34 : 0.18),
							radius: colorScheme == .dark ? 3.4 : 2.2,
							x: 0,
							y: 0
						)
						.overlay(alignment: .top) {
							Capsule()
								.fill(Color.white.opacity(colorScheme == .dark ? 0.28 : 0.34))
								.frame(height: 1)
								.padding(.horizontal, 1.2)
								.allowsHitTesting(false)
						}
						.overlay(alignment: .trailing) {
							Capsule()
								.fill(progressCapStyle)
								.frame(width: min(13, max(4, width)), height: 5)
								.opacity(progress > 0.04 ? 1 : 0)
								.allowsHitTesting(false)
						}
					Capsule()
						.strokeBorder(trackEdgeColor, lineWidth: 0.35)
						.allowsHitTesting(false)
				}
			}
			.frame(height: 5.2)
		}
		.font(PanelFont.usage)
		.lineLimit(1)
		.frame(height: 18)
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel("\(label) remaining \(remainingText), \(resetDisplay.accessibility)")
	}

	private var remainingText: String {
		guard let remainingPercent else {
			return "n/a"
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
		case .codexActive: return PanelPalette.activeGold(colorScheme)
		case .ready: return PanelPalette.usageMint(colorScheme)
		case .selected: return PanelPalette.actionBlue(colorScheme)
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
				color.opacity(colorScheme == .dark ? 0.98 : 0.78),
				color.opacity(colorScheme == .dark ? 0.82 : 0.64),
				color.opacity(colorScheme == .dark ? 0.58 : 0.48),
			],
			startPoint: .leading,
			endPoint: .trailing
		)
	}

	private var trackInsetStyle: LinearGradient {
		LinearGradient(
			colors: [
				Color.white.opacity(colorScheme == .dark ? 0.035 : 0.08),
				Color.white.opacity(0),
				Color.black.opacity(colorScheme == .dark ? 0.18 : 0.05),
			],
			startPoint: .top,
			endPoint: .bottom
		)
	}

	private var progressCapStyle: RadialGradient {
		RadialGradient(
			colors: [
				Color.white.opacity(colorScheme == .dark ? 0.7 : 0.6),
				color.opacity(colorScheme == .dark ? 0.5 : 0.36),
				Color.white.opacity(0),
			],
			center: .center,
			startRadius: 0,
			endRadius: 7
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
				short: "not reported",
				date: "",
				accessibility: "reset not reported"
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
			tint: PanelPalette.warning(colorScheme).opacity(0.12),
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
		HStack(spacing: 5) {
			Image(systemName: symbol)
				.font(PanelFont.summaryIcon)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.78 : 0.82))
				.frame(width: 11)

			VStack(alignment: .leading, spacing: 1) {
				Text(title)
					.font(PanelFont.summaryTitle)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
					.lineLimit(1)

				Text(value)
					.font(PanelFont.summaryValue)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

struct PanelPrimaryButtonView: View {
	let title: String
	let symbol: String
	let action: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Button(action: action) {
			Label(title, systemImage: symbol)
				.font(PanelFont.primaryButton)
				.foregroundStyle(
					colorScheme == .dark
						? Color(red: 0.86, green: 0.93, blue: 1).opacity(0.94)
						: PanelPalette.actionBlue(colorScheme)
				)
				.frame(maxWidth: .infinity, minHeight: 22)
				.contentShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
				.modernGlassSurface(
					cornerRadius: 9,
					tint: PanelPalette.addButtonTint(colorScheme),
					depth: .row
				)
				.overlay {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.strokeBorder(PanelPalette.addButtonStroke(colorScheme), lineWidth: 0.55)
						.allowsHitTesting(false)
				}
		}
		.buttonStyle(
			PanelInteractiveButtonStyle(
				hoverLift: 0.6,
				hoverScale: 1.004,
				pressedScale: 0.992,
				hoverShadowRadius: 3.5
			)
		)
		.help(title)
	}
}

struct PanelIconButtonView: View {
	let symbol: String
	let tint: Color
	let isActive: Bool
	let isDestructive: Bool
	let isDisabled: Bool
	let isSubtle: Bool
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
					tint: surfaceTint,
					depth: .control
				)
				.overlay {
					RoundedRectangle(cornerRadius: iconCornerRadius, style: .continuous)
						.strokeBorder(buttonStrokeColor, lineWidth: isDestructive ? 0.62 : 0.54)
						.allowsHitTesting(false)
				}
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
		if isSubtle {
			return tint.opacity(colorScheme == .dark ? 0.86 : 0.82)
		}
		return PanelPalette.actionBlue(colorScheme).opacity(colorScheme == .dark ? 0.88 : 0.86)
	}

	private var surfaceTint: Color {
		if isActive {
			return tint.opacity(colorScheme == .dark ? 0.22 : 0.2)
		}
		if isDisabled {
			return PanelPalette.controlTint(colorScheme).opacity(0.42)
		}
		if isDestructive {
			return colorScheme == .dark
				? tint.opacity(0.18)
				: Color(red: 1, green: 0.84, blue: 0.86).opacity(0.72)
		}
		return PanelPalette.controlTint(colorScheme).opacity(isSubtle ? 0.94 : 1)
	}

	private var usesSurface: Bool {
		true
	}

	private var buttonStrokeColor: Color {
		if isDestructive {
			return tint.opacity(colorScheme == .dark ? 0.38 : 0.34)
		}
		if isActive {
			return tint.opacity(colorScheme == .dark ? 0.4 : 0.34)
		}

		return PanelPalette.controlStroke(colorScheme).opacity(isSubtle ? 1 : 0.92)
	}

	private var iconCornerRadius: CGFloat {
		min(8.5, max(7, size * 0.36))
	}
}

struct PanelIconLabelView: View {
	let symbol: String
	let tint: Color
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Image(systemName: symbol)
			.font(PanelFont.footerIcon)
			.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.84 : 0.86))
			.frame(width: 24, height: 24)
			.modernGlassSurface(
				cornerRadius: 7,
				tint: PanelPalette.controlTint(colorScheme),
				depth: .control
			)
			.overlay {
				RoundedRectangle(cornerRadius: 7, style: .continuous)
					.strokeBorder(PanelPalette.controlStroke(colorScheme), lineWidth: 0.5)
					.allowsHitTesting(false)
			}
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
		tint: Color? = nil,
		depth: GlassSurfaceDepth = .section
	) -> some View {
		modifier(
			ModernGlassSurfaceModifier(
				cornerRadius: cornerRadius,
				tint: tint,
				depth: depth
			)
		)
	}
}

private struct ModernGlassSurfaceModifier: ViewModifier {
	@Environment(\.colorScheme) private var colorScheme
	let cornerRadius: CGFloat
	let tint: Color?
	let depth: GlassSurfaceDepth

	@ViewBuilder
	func body(content: Content) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		if #available(macOS 26.0, *) {
			content
				.background {
					shape.fill(surfaceFill)
				}
				.glassEffect(
					configuredGlass,
					in: shape
				)
				.glassSurfaceFinish(
					cornerRadius: cornerRadius,
					depth: depth,
					colorScheme: colorScheme
				)
		} else {
			content
				.background {
					shape
						.fill(materialStyle)
					shape
						.fill(surfaceFill)
				}
				.glassSurfaceFinish(
					cornerRadius: cornerRadius,
					depth: depth,
					colorScheme: colorScheme
				)
		}
	}

	@available(macOS 26.0, *)
	private var configuredGlass: Glass {
		var glass = Glass.regular
		if let tint {
			glass = glass.tint(tint)
		}
		if depth == .control {
			glass = glass.interactive()
		}

		return glass
	}

	private var materialStyle: AnyShapeStyle {
		switch depth {
		case .panel:
			return AnyShapeStyle(.regularMaterial)
		case .section, .row:
			return AnyShapeStyle(.thinMaterial)
		case .control:
			return colorScheme == .dark ? AnyShapeStyle(.thinMaterial) : AnyShapeStyle(.ultraThinMaterial)
		}
	}

	private var surfaceFill: Color {
		if let tint {
			return tint
		}

		if colorScheme == .dark {
			switch depth {
			case .panel:
				return Color(hue: 0.59, saturation: 0.4, brightness: 0.38, opacity: 0.3)
			case .section:
				return Color(hue: 0.59, saturation: 0.25, brightness: 0.5, opacity: 0.12)
			case .row:
				return Color(hue: 0.59, saturation: 0.24, brightness: 0.52, opacity: 0.13)
			case .control:
				return Color(hue: 0.59, saturation: 0.27, brightness: 0.56, opacity: 0.115)
			}
		}

		switch depth {
		case .panel:
			return Color(
				hue: 0.6,
				saturation: 0.18,
				brightness: 1,
				opacity: 0.1
			)
		case .section:
			return Color(
				hue: 0.6,
				saturation: 0.08,
				brightness: 1,
				opacity: 0.12
			)
		case .row:
			return Color(
				hue: 0.6,
				saturation: 0.07,
				brightness: 1,
				opacity: 0.135
			)
		case .control:
			return Color.white.opacity(0.145)
		}
	}
}

private extension View {
	func glassSurfaceFinish(
		cornerRadius: CGFloat,
		depth: GlassSurfaceDepth,
		colorScheme: ColorScheme
	) -> some View {
		let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

		return self
			.background {
				shape
					.fill(castShadowColor(for: depth, colorScheme: colorScheme))
					.blur(radius: castShadowBlur(for: depth))
					.offset(
						x: castShadowXOffset(for: depth),
						y: castShadowOffset(for: depth)
					)
					.opacity(castShadowOpacity(for: depth, colorScheme: colorScheme))
					.allowsHitTesting(false)
			}
			.background(alignment: .bottom) {
				shape
					.fill(contactShadowGradient(for: depth, colorScheme: colorScheme))
					.frame(height: contactShadowHeight(for: depth))
					.blur(radius: contactShadowBlur(for: depth))
					.offset(y: contactShadowOffset(for: depth))
					.opacity(contactShadowOpacity(for: depth, colorScheme: colorScheme))
					.allowsHitTesting(false)
			}
			.overlay {
				shape
					.fill(surfaceLiftGradient(for: depth, colorScheme: colorScheme))
					.blendMode(colorScheme == .dark ? .screen : .softLight)
					.allowsHitTesting(false)
			}
			.overlay {
				shape
					.fill(sheenGradient(for: depth, colorScheme: colorScheme))
					.blendMode(.screen)
					.allowsHitTesting(false)
			}
			.overlay {
				shape
					.strokeBorder(
						edgeGradient(for: depth, colorScheme: colorScheme),
						lineWidth: edgeWidth(for: depth)
					)
					.allowsHitTesting(false)
			}
			.shadow(
				color: ambientShadowColor(for: depth, colorScheme: colorScheme),
				radius: ambientShadowRadius(for: depth),
				x: ambientShadowXOffset(for: depth),
				y: ambientShadowOffset(for: depth)
			)
			.shadow(
				color: keyShadowColor(for: depth, colorScheme: colorScheme),
				radius: keyShadowRadius(for: depth),
				x: keyShadowXOffset(for: depth),
				y: keyShadowOffset(for: depth)
			)
	}

	private func edgeGradient(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> LinearGradient {
		LinearGradient(
			colors: [
				Color.white.opacity(edgeHighlightOpacity(for: depth, colorScheme: colorScheme)),
				Color.white.opacity(edgeMidOpacity(for: depth, colorScheme: colorScheme)),
				Color.black.opacity(edgeShadowOpacity(for: depth, colorScheme: colorScheme)),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private func surfaceLiftGradient(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> LinearGradient {
		LinearGradient(
			colors: [
				Color.white.opacity(surfaceTopGlowOpacity(for: depth, colorScheme: colorScheme)),
				Color.white.opacity(0),
				Color.black.opacity(surfaceBottomShadeOpacity(for: depth, colorScheme: colorScheme)),
			],
			startPoint: .top,
			endPoint: .bottom
		)
	}

	private func sheenGradient(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> LinearGradient {
		LinearGradient(
			colors: [
				Color.white.opacity(sheenOpacity(for: depth, colorScheme: colorScheme)),
				Color.white.opacity(0),
				Color.black.opacity(colorScheme == .dark ? 0.035 : 0.02),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private func contactShadowGradient(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> LinearGradient {
		let color = contactShadowColor(for: depth, colorScheme: colorScheme)
		return LinearGradient(
			colors: [
				color.opacity(0),
				color,
				color.opacity(0.35),
				color.opacity(0),
			],
			startPoint: .top,
			endPoint: .bottom
		)
	}

	private func contactShadowColor(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Color {
		if colorScheme == .dark {
			switch depth {
			case .panel:
				return Color.black
			case .section, .row:
				return Color(red: 0.01, green: 0.04, blue: 0.08)
			case .control:
				return Color.black
			}
		}

		switch depth {
		case .panel:
			return Color(red: 0.08, green: 0.18, blue: 0.3)
		case .section, .row:
			return Color(red: 0.12, green: 0.28, blue: 0.42)
		case .control:
			return Color(red: 0.1, green: 0.24, blue: 0.38)
		}
	}

	private func castShadowColor(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Color {
		if colorScheme == .dark {
			switch depth {
			case .panel:
				return Color.black
			case .section, .row:
				return Color(red: 0.01, green: 0.035, blue: 0.07)
			case .control:
				return Color.black
			}
		}

		switch depth {
		case .panel:
			return Color(red: 0.08, green: 0.18, blue: 0.3)
		case .section, .row:
			return Color(red: 0.1, green: 0.24, blue: 0.38)
		case .control:
			return Color(red: 0.08, green: 0.2, blue: 0.34)
		}
	}

	private func castShadowOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel:
			return colorScheme == .dark ? 0.13 : 0.085
		case .section:
			return colorScheme == .dark ? 0.1 : 0.18
		case .row:
			return colorScheme == .dark ? 0.16 : 0.27
		case .control:
			return colorScheme == .dark ? 0.14 : 0.2
		}
	}

	private func castShadowBlur(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 16
		case .section: return 10
		case .row: return 12
		case .control: return 7
		}
	}

	private func castShadowXOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 0
		case .section: return 0.5
		case .row: return 0.7
		case .control: return 0.4
		}
	}

	private func castShadowOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 8
		case .section: return 6
		case .row: return 8
		case .control: return 4
		}
	}

	private func contactShadowOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel:
			return colorScheme == .dark ? 0.1 : 0.085
		case .section:
			return colorScheme == .dark ? 0.13 : 0.32
		case .row:
			return colorScheme == .dark ? 0.18 : 0.42
		case .control:
			return colorScheme == .dark ? 0.14 : 0.24
		}
	}

	private func contactShadowHeight(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 16
		case .section: return 10
		case .row: return 12
		case .control: return 8
		}
	}

	private func contactShadowBlur(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 7
		case .section: return 3.2
		case .row: return 3.6
		case .control: return 3
		}
	}

	private func contactShadowOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 8
		case .section: return 5.8
		case .row: return 6.8
		case .control: return 4
		}
	}

	private func surfaceTopGlowOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel:
			return colorScheme == .dark ? 0.025 : 0.045
		case .section:
			return colorScheme == .dark ? 0.038 : 0.085
		case .row:
			return colorScheme == .dark ? 0.044 : 0.095
		case .control:
			return colorScheme == .dark ? 0.055 : 0.11
		}
	}

	private func surfaceBottomShadeOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel:
			return colorScheme == .dark ? 0.012 : 0.018
		case .section:
			return colorScheme == .dark ? 0.018 : 0.03
		case .row:
			return colorScheme == .dark ? 0.022 : 0.04
		case .control:
			return colorScheme == .dark ? 0.02 : 0.035
		}
	}

	private func sheenOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel: return colorScheme == .dark ? 0.045 : 0.1
		case .section: return colorScheme == .dark ? 0.04 : 0.11
		case .row: return colorScheme == .dark ? 0.046 : 0.12
		case .control: return colorScheme == .dark ? 0.058 : 0.13
		}
	}

	private func edgeWidth(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 0.65
		case .section: return 0.48
		case .row: return 0.52
		case .control: return 0.45
		}
	}

	private func edgeHighlightOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel: return colorScheme == .dark ? 0.17 : 0.32
		case .section: return colorScheme == .dark ? 0.14 : 0.34
		case .row: return colorScheme == .dark ? 0.17 : 0.42
		case .control: return colorScheme == .dark ? 0.2 : 0.38
		}
	}

	private func edgeMidOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel: return colorScheme == .dark ? 0.045 : 0.105
		case .section: return colorScheme == .dark ? 0.028 : 0.07
		case .row: return colorScheme == .dark ? 0.03 : 0.065
		case .control: return colorScheme == .dark ? 0.04 : 0.09
		}
	}

	private func edgeShadowOpacity(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Double {
		switch depth {
		case .panel: return colorScheme == .dark ? 0.085 : 0.075
		case .section: return colorScheme == .dark ? 0.045 : 0.075
		case .row: return colorScheme == .dark ? 0.045 : 0.11
		case .control: return colorScheme == .dark ? 0.06 : 0.09
		}
	}

	private func ambientShadowColor(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Color {
		let opacity: Double
		switch depth {
		case .panel:
			opacity = colorScheme == .dark ? 0.24 : 0.065
		case .section:
			opacity = colorScheme == .dark ? 0.09 : 0.095
		case .row:
			opacity = colorScheme == .dark ? 0.18 : 0.15
		case .control:
			opacity = colorScheme == .dark ? 0.1 : 0.058
		}

		return Color.black.opacity(opacity)
	}

	private func keyShadowColor(for depth: GlassSurfaceDepth, colorScheme: ColorScheme) -> Color {
		let opacity: Double
		switch depth {
		case .panel:
			opacity = colorScheme == .dark ? 0.07 : 0.028
		case .section:
			opacity = colorScheme == .dark ? 0.065 : 0.055
		case .row:
			opacity = colorScheme == .dark ? 0.08 : 0.07
		case .control:
			opacity = colorScheme == .dark ? 0.07 : 0.046
		}

		return Color(red: 0.34, green: 0.67, blue: 1).opacity(opacity)
	}

	private func ambientShadowRadius(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 15
		case .section: return 9
		case .row: return 13
		case .control: return 5.5
		}
	}

	private func ambientShadowXOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 0
		case .section: return 0.6
		case .row: return 0.8
		case .control: return 0.4
		}
	}

	private func ambientShadowOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 6
		case .section: return 4
		case .row: return 8
		case .control: return 2
		}
	}

	private func keyShadowRadius(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 5
		case .section: return 4
		case .row: return 4.5
		case .control: return 3
		}
	}

	private func keyShadowXOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 0
		case .section: return 0.3
		case .row: return 0.4
		case .control: return 0.2
		}
	}

	private func keyShadowOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 2
		case .section: return 2
		case .row: return 2
		case .control: return 1
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

	func matchesSelector(_ value: String) -> Bool {
		let selector = value.trimmingCharacters(in: .whitespacesAndNewlines)
		return selector == email || selector == accountFingerprint || selector == self.selector
	}

	func statusDisplayColor(colorScheme: ColorScheme) -> Color {
		switch statusTone {
		case .codexActive:
			return PanelPalette.activeGold(colorScheme)
		case .ready:
			return PanelPalette.usageMint(colorScheme)
		case .selected:
			return PanelPalette.actionBlue(colorScheme)
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

	var hasVisibleMetadata: Bool {
		planLabel != nil || compactHealthLabel != nil
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
