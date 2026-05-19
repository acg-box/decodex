import SwiftUI

struct AccountPanelView: View {
	@ObservedObject var store: AccountStore
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
		VStack(alignment: .leading, spacing: 8) {
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
		.frame(width: 318)
		.padding(10)
		.modernGlassSurface(
			cornerRadius: 18,
			tint: Color.accentColor.opacity(0.035),
			depth: .panel
		)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
	}

	private var header: some View {
		HStack(alignment: .center, spacing: 8) {
			Image(systemName: store.menuSymbol)
				.font(.system(size: 15, weight: .semibold))
				.foregroundStyle(Color.accentColor)
				.frame(width: 28, height: 28)
				.modernGlassSurface(
					cornerRadius: 9,
					tint: Color.accentColor.opacity(0.16),
					depth: .control
				)

			VStack(alignment: .leading, spacing: 2) {
				Text("Decodex")
					.font(.system(size: 13, weight: .semibold))
				Text(headerSubtitle)
					.font(.caption2)
					.foregroundStyle(.secondary)
					.lineLimit(1)
			}

			Spacer()

			HStack(spacing: 4) {
				PanelIconButtonView(
					symbol: emailsHidden ? "eye.slash" : "eye",
					tint: .secondary,
					isActive: false,
					action: {
						accountPrivacy = emailsHidden ? AccountPrivacy.visibleValue : AccountPrivacy.hiddenValue
					},
					help: emailsHidden ? "Show account emails" : "Hide account emails"
				)

				PanelIconButtonView(
					symbol: store.isRefreshing ? "arrow.triangle.2.circlepath.circle" : "arrow.clockwise",
					tint: .secondary,
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
				symbol: "bolt.fill",
				tint: .yellow
			)

			Divider()
				.opacity(0.5)
				.padding(.vertical, 3)

			SummaryTileView(
				title: "Runs",
				value: decodexModeLabel,
				symbol: hasFixedSelection ? "pin.fill" : "arrow.triangle.branch",
				tint: hasFixedSelection ? .accentColor : .secondary
			)
		}
		.padding(.horizontal, 7)
		.padding(.vertical, 5)
		.modernGlassSurface(
			cornerRadius: 11,
			tint: Color.primary.opacity(0.045),
			depth: .section
		)
	}

	private var emptyState: some View {
		VStack(alignment: .leading, spacing: 6) {
			Image(systemName: "person.crop.circle.badge.plus")
				.font(.system(size: 19))
				.foregroundStyle(.secondary)
			Text("No accounts in the local pool")
				.font(.subheadline.weight(.semibold))
			Text("Add a ChatGPT login before switching the Codex auth file.")
				.font(.caption)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(8)
		.modernGlassSurface(cornerRadius: 10, depth: .section)
	}

	private var loadingState: some View {
		HStack(spacing: 7) {
			ProgressView()
				.controlSize(.small)
			Text("Loading accounts")
				.font(.subheadline.weight(.semibold))
			Spacer()
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(8)
		.modernGlassSurface(cornerRadius: 10, depth: .section)
	}

	private var accountList: some View {
		ScrollView {
			LazyVStack(spacing: 4) {
				ForEach(store.accounts) { account in
					AccountRowView(
						account: account,
						emailsHidden: emailsHidden,
						useInCodex: {
							Task {
								await store.useInCodex(account)
							}
						},
						pinForDecodex: {
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
		.frame(height: accountListHeight)
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

			PanelIconButtonView(
				symbol: "arrow.triangle.branch",
				tint: .accentColor,
				isActive: false,
				isDisabled: !hasFixedSelection,
				action: {
					Task {
						await store.clearSelection()
					}
				},
				help: "Return Decodex runs to balanced selection"
			)

			SettingsLink {
				PanelIconLabelView(symbol: "gearshape", tint: .secondary)
			}
			.buttonStyle(.plain)
			.help("Settings")
		}
	}

	private var codexAuthLabel: String {
		guard let auth = store.accountList?.codexAuth else {
			return "No Codex auth"
		}

		if emailsHidden {
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
				return account(matching: selector).map(AccountDisplay.alias) ?? "Account"
			}

			if selector.contains("@") {
				return AccountDisplay.compactEmail(selector)
			}

			return AccountDisplay.compactIdentity(selector)
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
			total + (account.hasUsageWindowData ? 64 : 44)
		}
		let spacing = CGFloat(max(store.accounts.count - 1, 0)) * 4 + 2

		return min(
			rows + spacing,
			248
		)
	}

	private var headerSubtitle: String {
		let count = store.accounts.count
		let accountLabel = "\(count) account\(count == 1 ? "" : "s")"
		return hasFixedSelection ? "\(accountLabel) / pinned runs" : "\(accountLabel) / balanced runs"
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
	let pinForDecodex: () -> Void
	let logout: () -> Void

	var body: some View {
		HStack(spacing: 7) {
			AccountAvatarView(account: account, title: displayName)

			VStack(alignment: .leading, spacing: 2) {
				HStack(spacing: 4) {
					Text(displayName)
						.font(.caption.weight(.semibold))
						.lineLimit(1)
						.truncationMode(.middle)
						.layoutPriority(1)

					if account.codexActive {
						StatusMarkerView(symbol: "bolt.fill", tint: .yellow)
					}

					if account.selected {
						StatusMarkerView(symbol: "pin.fill", tint: .accentColor)
					}
				}

				HStack(spacing: 4) {
					Text(detailLabel)
						.lineLimit(1)
						.truncationMode(.middle)
					Text("·")
					if let planLabel = account.planLabel {
						Text(planLabel)
							.lineLimit(1)
						Text("·")
					}
					Text(account.statusLabel)
						.lineLimit(1)
				}
				.font(.caption2)
				.foregroundStyle(.secondary)

				if account.hasUsageWindowData {
					HStack(spacing: 4) {
						AccountUsageBadgeView(
							label: account.windowLabel(seconds: account.primaryWindowSeconds),
							remainingPercent: account.primaryRemainingPercent,
							tone: account.usageTone(
								remainingPercent: account.primaryRemainingPercent
							)
						)
						AccountUsageBadgeView(
							label: account.windowLabel(seconds: account.secondaryWindowSeconds),
							remainingPercent: account.secondaryRemainingPercent,
							tone: account.usageTone(
								remainingPercent: account.secondaryRemainingPercent
							)
						)
					}
				}
			}
			.frame(maxWidth: .infinity, alignment: .leading)

			Spacer(minLength: 4)

			HStack(spacing: 3) {
				PanelIconButtonView(
					symbol: account.codexActive ? "bolt.fill" : "bolt",
					tint: .yellow,
					isActive: account.codexActive,
					size: 22,
					action: useInCodex,
					help: account.codexActive ? "Already active in Codex" : "Use in Codex"
				)

				PanelIconButtonView(
					symbol: account.selected ? "pin.fill" : "pin",
					tint: .accentColor,
					isActive: account.selected,
					size: 22,
					action: pinForDecodex,
					help: account.selected ? "Return Decodex to balanced selection" : "Pin for Decodex runs"
				)

				PanelIconButtonView(
					symbol: "trash",
					tint: .red,
					isActive: false,
					isDestructive: true,
					size: 22,
					action: logout,
					help: "Remove account"
				)
			}
		}
		.padding(.vertical, 5)
		.padding(.leading, 6)
		.padding(.trailing, 6)
		.modernGlassSurface(
			cornerRadius: 11,
			tint: account.rowTint,
			depth: .row,
			interactive: true
		)
	}

	private var displayName: String {
		account.panelDisplayName(emailsHidden: emailsHidden)
	}

	private var detailLabel: String {
		account.panelDetailLabel(emailsHidden: emailsHidden)
	}
}

struct AccountUsageBadgeView: View {
	let label: String
	let remainingPercent: Int?
	let tone: AccountTone

	var body: some View {
		HStack(spacing: 3) {
			Text(label)
				.font(.caption2.weight(.medium))
				.foregroundStyle(.secondary)
			Text(remainingText)
				.font(.caption2.monospacedDigit().weight(.semibold))
		}
		.padding(.horizontal, 4)
		.padding(.vertical, 1)
		.modernGlassSurface(
			cornerRadius: 7,
			tint: color.opacity(0.16),
			depth: .badge
		)
	}

	private var remainingText: String {
		guard let remainingPercent else {
			return "n/a"
		}

		return "\(remainingPercent)%"
	}

	private var color: Color {
		switch tone {
		case .codexActive: return .yellow
		case .ready: return .green
		case .selected: return .accentColor
		case .warning: return .yellow
		case .danger: return .red
		case .neutral: return .secondary
		}
	}
}

struct NoticeView: View {
	let text: String

	var body: some View {
		HStack(alignment: .top, spacing: 7) {
			Image(systemName: "exclamationmark.triangle")
				.foregroundStyle(.yellow)
			Text(text)
				.font(.caption)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)
		}
		.padding(8)
		.modernGlassSurface(
			cornerRadius: 10,
			tint: .yellow.opacity(0.12),
			depth: .section
		)
	}
}

struct SummaryTileView: View {
	let title: String
	let value: String
	let symbol: String
	let tint: Color

	var body: some View {
		HStack(spacing: 5) {
			Image(systemName: symbol)
				.font(.caption2.weight(.semibold))
				.foregroundStyle(tint)
				.frame(width: 11)

			VStack(alignment: .leading, spacing: 1) {
				Text(title)
					.font(.caption2.weight(.medium))
					.foregroundStyle(.secondary)
					.lineLimit(1)

				Text(value)
					.font(.caption2.weight(.semibold))
					.lineLimit(1)
					.truncationMode(.middle)
			}
		}
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

struct StatusMarkerView: View {
	let symbol: String
	let tint: Color

	var body: some View {
		Image(systemName: symbol)
			.font(.system(size: 9, weight: .bold))
			.foregroundStyle(tint)
			.frame(width: 14, height: 14)
			.modernGlassSurface(cornerRadius: 7, tint: tint.opacity(0.16), depth: .badge)
	}
}

struct PanelPrimaryButtonView: View {
	let title: String
	let symbol: String
	let action: () -> Void

	var body: some View {
		Button(action: action) {
			Label(title, systemImage: symbol)
				.font(.caption.weight(.semibold))
				.frame(maxWidth: .infinity, minHeight: 24)
				.contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
		}
		.buttonStyle(.plain)
		.foregroundStyle(.primary)
		.modernGlassSurface(
			cornerRadius: 9,
			tint: Color.accentColor.opacity(0.2),
			depth: .control,
			interactive: true
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
	let size: CGFloat
	let action: () -> Void
	let help: String

	init(
		symbol: String,
		tint: Color,
		isActive: Bool,
		isDestructive: Bool = false,
		isDisabled: Bool = false,
		size: CGFloat = 24,
		action: @escaping () -> Void,
		help: String
	) {
		self.symbol = symbol
		self.tint = tint
		self.isActive = isActive
		self.isDestructive = isDestructive
		self.isDisabled = isDisabled
		self.size = size
		self.action = action
		self.help = help
	}

	var body: some View {
		Button(action: action) {
			Image(systemName: symbol)
				.font(.system(size: 10, weight: .semibold))
				.foregroundStyle(foregroundColor)
				.frame(width: size, height: size)
				.contentShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
		}
		.buttonStyle(.plain)
		.disabled(isDisabled)
		.modernGlassSurface(
			cornerRadius: 7,
			tint: surfaceTint,
			depth: .control,
			interactive: !isDisabled
		)
		.help(help)
	}

	private var foregroundColor: Color {
		if isDisabled {
			return Color.secondary.opacity(0.45)
		}
		if usesTint {
			return tint
		}
		return .secondary
	}

	private var surfaceTint: Color {
		if isDisabled {
			return Color.primary.opacity(0.02)
		}
		if usesTint {
			return tint.opacity(isDestructive ? 0.07 : 0.11)
		}
		return Color.primary.opacity(0.03)
	}

	private var usesTint: Bool {
		isActive || isDestructive
	}
}

struct PanelIconLabelView: View {
	let symbol: String
	let tint: Color

	var body: some View {
		Image(systemName: symbol)
			.font(.system(size: 11, weight: .semibold))
			.foregroundStyle(tint)
			.frame(width: 24, height: 24)
			.modernGlassSurface(
				cornerRadius: 7,
				tint: Color.primary.opacity(0.03),
				depth: .control,
				interactive: true
			)
	}
}

struct AccountAvatarView: View {
	let account: CodexAccount
	let title: String

	var body: some View {
		ZStack(alignment: .bottomTrailing) {
			Circle()
				.fill(account.statusColor.opacity(0.12))
				.overlay {
					Circle()
						.strokeBorder(account.statusColor.opacity(0.22), lineWidth: 1)
				}

			Text(AccountDisplay.initials(from: title))
				.font(.caption2.weight(.semibold))
				.foregroundStyle(.primary)

			Circle()
				.fill(account.statusColor)
				.frame(width: 6, height: 6)
				.overlay {
					Circle()
						.stroke(.background, lineWidth: 1.5)
				}
		}
		.frame(width: 22, height: 22)
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
		alias(
			forIdentity: account.accountFingerprint.isEmpty ? account.selector : account.accountFingerprint
		)
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

	static func initials(from value: String) -> String {
		let components = value
			.split { !$0.isLetter && !$0.isNumber }
			.prefix(2)
			.compactMap(\.first)
		let initials = String(components).uppercased()

		return initials.isEmpty ? "?" : initials
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
	case badge
}

private extension View {
	@ViewBuilder
	func modernGlassSurface(
		cornerRadius: CGFloat,
		tint: Color? = nil,
		depth: GlassSurfaceDepth = .section,
		interactive: Bool = false
	) -> some View {
		if #available(macOS 26.0, *) {
			self.glassEffect(
				configuredGlass(tint: tint, interactive: interactive),
				in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
			)
			.glassSurfaceFinish(cornerRadius: cornerRadius, depth: depth)
		} else {
			self
				.background(
					tint.map { AnyShapeStyle($0) } ?? AnyShapeStyle(.regularMaterial),
					in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
				)
				.glassSurfaceFinish(cornerRadius: cornerRadius, depth: depth)
		}
	}

	func glassSurfaceFinish(cornerRadius: CGFloat, depth: GlassSurfaceDepth) -> some View {
		self
			.overlay {
				RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
					.strokeBorder(edgeGradient(for: depth), lineWidth: edgeWidth(for: depth))
			}
			.shadow(
				color: Color.black.opacity(shadowOpacity(for: depth)),
				radius: shadowRadius(for: depth),
				x: 0,
				y: shadowOffset(for: depth)
			)
	}

	@available(macOS 26.0, *)
	private func configuredGlass(tint: Color?, interactive: Bool) -> Glass {
		var glass = Glass.regular
		if let tint {
			glass = glass.tint(tint)
		}
		if interactive {
			glass = glass.interactive()
		}

		return glass
	}

	private func edgeGradient(for depth: GlassSurfaceDepth) -> LinearGradient {
		LinearGradient(
			colors: [
				Color.white.opacity(edgeHighlightOpacity(for: depth)),
				Color.white.opacity(edgeMidOpacity(for: depth)),
				Color.black.opacity(edgeShadowOpacity(for: depth)),
			],
			startPoint: .topLeading,
			endPoint: .bottomTrailing
		)
	}

	private func edgeWidth(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 0.9
		case .section: return 0.65
		case .row: return 0.55
		case .control: return 0.6
		case .badge: return 0.45
		}
	}

	private func edgeHighlightOpacity(for depth: GlassSurfaceDepth) -> Double {
		switch depth {
		case .panel: return 0.34
		case .section: return 0.24
		case .row: return 0.2
		case .control: return 0.28
		case .badge: return 0.18
		}
	}

	private func edgeMidOpacity(for depth: GlassSurfaceDepth) -> Double {
		switch depth {
		case .panel: return 0.1
		case .section: return 0.08
		case .row: return 0.06
		case .control: return 0.09
		case .badge: return 0.05
		}
	}

	private func edgeShadowOpacity(for depth: GlassSurfaceDepth) -> Double {
		switch depth {
		case .panel: return 0.18
		case .section: return 0.12
		case .row: return 0.1
		case .control: return 0.12
		case .badge: return 0.08
		}
	}

	private func shadowOpacity(for depth: GlassSurfaceDepth) -> Double {
		switch depth {
		case .panel: return 0.18
		case .section: return 0.1
		case .row: return 0.07
		case .control: return 0.08
		case .badge: return 0.04
		}
	}

	private func shadowRadius(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 18
		case .section: return 8
		case .row: return 5
		case .control: return 5
		case .badge: return 2
		}
	}

	private func shadowOffset(for depth: GlassSurfaceDepth) -> CGFloat {
		switch depth {
		case .panel: return 10
		case .section: return 4
		case .row: return 2
		case .control: return 2
		case .badge: return 1
		}
	}
}

private extension CodexAccount {
	func panelDisplayName(emailsHidden: Bool) -> String {
		if emailsHidden {
			return AccountDisplay.alias(for: self)
		}

		return AccountDisplay.compactEmail(displayName)
	}

	func panelDetailLabel(emailsHidden: Bool) -> String {
		if emailsHidden {
			return AccountDisplay.compactIdentity(accountFingerprint)
		}

		return accountFingerprint
	}

	func matchesSelector(_ value: String) -> Bool {
		let selector = value.trimmingCharacters(in: .whitespacesAndNewlines)
		return selector == email || selector == accountFingerprint || selector == self.selector
	}

	var statusColor: Color {
		switch statusTone {
		case .codexActive: return .yellow
		case .ready: return .green
		case .selected: return .accentColor
		case .warning: return .yellow
		case .danger: return .red
		case .neutral: return .secondary
		}
	}

	var rowTint: Color? {
		if codexActive {
			return Color.yellow.opacity(0.06)
		}
		if selected {
			return Color.accentColor.opacity(0.06)
		}

		return nil
	}
}
