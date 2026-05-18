import SwiftUI

struct AccountPanelView: View {
	@ObservedObject var store: AccountStore
	@State private var pendingLogout: CodexAccount?
	@State private var loginPresented = false
	@AppStorage("decodex.operator.accountPrivacy") private var accountPrivacy = AccountPrivacy.hiddenValue

	var body: some View {
		Group {
			if #available(macOS 26.0, *) {
				GlassEffectContainer(spacing: 10) {
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
		VStack(alignment: .leading, spacing: 12) {
			header
			accountSummary

			if let notice = store.notice {
				NoticeView(text: notice)
			}

			if store.accounts.isEmpty {
				emptyState
			} else {
				accountList
			}

			footer
		}
		.frame(width: 324)
		.padding(14)
		.modernGlassSurface(cornerRadius: 22)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
	}

	private var header: some View {
		HStack(alignment: .center, spacing: 12) {
			Image(systemName: store.menuSymbol)
				.font(.system(size: 22, weight: .semibold))
				.frame(width: 30, height: 30)
				.foregroundStyle(.primary)

			VStack(alignment: .leading, spacing: 3) {
				Text("Decodex")
					.font(.system(size: 15, weight: .semibold))
				Text("\(store.accounts.count) account\(store.accounts.count == 1 ? "" : "s")")
					.font(.caption2)
					.foregroundStyle(.secondary)
					.lineLimit(1)
			}

			Spacer()

			HStack(spacing: 6) {
				Button {
					accountPrivacy = emailsHidden ? AccountPrivacy.visibleValue : AccountPrivacy.hiddenValue
				} label: {
					Image(systemName: emailsHidden ? "eye.slash" : "eye")
						.frame(width: 18, height: 18)
				}
				.iconPanelButtonStyle()
				.help(emailsHidden ? "Show account emails" : "Hide account emails")

				Button {
					Task {
						await store.refresh()
					}
				} label: {
					Image(systemName: store.isRefreshing ? "arrow.triangle.2.circlepath.circle" : "arrow.clockwise")
						.frame(width: 18, height: 18)
				}
				.iconPanelButtonStyle()
				.help("Refresh")
				.disabled(store.isRefreshing)
			}
		}
	}

	private var accountSummary: some View {
		VStack(spacing: 7) {
			SummaryRowView(
				title: "Codex",
				value: codexAuthLabel,
				symbol: "bolt.fill",
				tint: .yellow
			)
			SummaryRowView(
				title: "Decodex runs",
				value: decodexModeLabel,
				symbol: hasFixedSelection ? "pin.fill" : "arrow.triangle.branch",
				tint: hasFixedSelection ? .accentColor : .secondary
			)
		}
		.padding(10)
		.modernGlassSurface(cornerRadius: 14, tint: .primary.opacity(0.035))
	}

	private var emptyState: some View {
		VStack(alignment: .leading, spacing: 9) {
			Image(systemName: "person.crop.circle.badge.plus")
				.font(.system(size: 24))
				.foregroundStyle(.secondary)
			Text("No accounts in the local pool")
				.font(.subheadline.weight(.semibold))
			Text("Add a ChatGPT login before switching the Codex auth file.")
				.font(.caption)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)
		}
		.frame(maxWidth: .infinity, alignment: .leading)
		.padding(12)
		.modernGlassSurface(cornerRadius: 14)
	}

	private var accountList: some View {
		ScrollView {
			LazyVStack(spacing: 6) {
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
		HStack(spacing: 8) {
			Button {
				loginPresented = true
			} label: {
				Label("Add Login", systemImage: "plus.circle")
			}
			.primaryPanelButtonStyle()

			Button {
				Task {
					await store.clearSelection()
				}
			} label: {
				Label("Balanced", systemImage: "arrow.triangle.branch")
			}
			.secondaryPanelButtonStyle()
			.disabled(!hasFixedSelection)

			Spacer()

			SettingsLink {
				Image(systemName: "gearshape")
					.frame(width: 18, height: 18)
			}
			.iconPanelButtonStyle()
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
		min(
			CGFloat(store.accounts.count) * 50 + CGFloat(max(store.accounts.count - 1, 0)) * 6 + 2,
			286
		)
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
		HStack(spacing: 8) {
			Button(action: useInCodex) {
				HStack(spacing: 11) {
					AccountAvatarView(account: account, title: displayName)

					VStack(alignment: .leading, spacing: 4) {
						Text(displayName)
							.font(.subheadline.weight(.semibold))
							.lineLimit(1)
							.truncationMode(.middle)
						HStack(spacing: 6) {
							Text(detailLabel)
								.lineLimit(1)
								.truncationMode(.middle)
							Text("·")
							Text(account.statusLabel)
								.lineLimit(1)
						}
						.font(.caption)
						.foregroundStyle(.secondary)
					}

					Spacer()

					if account.codexActive {
						StatusPillView(title: "Active", symbol: "bolt.fill", tint: .yellow)
					} else {
						Text("Use")
							.font(.caption.weight(.semibold))
							.foregroundStyle(.secondary)
					}
				}
				.padding(.vertical, 6)
				.padding(.leading, 8)
				.padding(.trailing, 10)
				.contentShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
			}
			.buttonStyle(.plain)
			.help("Use in Codex")

			Menu {
				Button(action: pinForDecodex) {
					Label(
						account.selected ? "Use balanced Decodex selection" : "Pin for Decodex runs",
						systemImage: account.selected ? "pin.slash" : "pin"
					)
				}

				Divider()

				Button(role: .destructive, action: logout) {
					Label("Remove account", systemImage: "rectangle.portrait.and.arrow.right")
				}
			} label: {
				Image(systemName: "ellipsis")
					.frame(width: 18, height: 18)
			}
			.menuStyle(.button)
			.buttonStyle(.borderless)
			.help("More actions")
		}
		.padding(3)
		.modernGlassSurface(
			cornerRadius: 15,
			tint: account.rowTint,
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

struct NoticeView: View {
	let text: String

	var body: some View {
		HStack(alignment: .top, spacing: 8) {
			Image(systemName: "exclamationmark.triangle")
				.foregroundStyle(.yellow)
			Text(text)
				.font(.caption)
				.foregroundStyle(.secondary)
				.fixedSize(horizontal: false, vertical: true)
		}
		.padding(10)
		.modernGlassSurface(cornerRadius: 14, tint: .yellow.opacity(0.08))
	}
}

struct SummaryRowView: View {
	let title: String
	let value: String
	let symbol: String
	let tint: Color

	var body: some View {
		HStack(spacing: 8) {
			Image(systemName: symbol)
				.font(.caption.weight(.semibold))
				.foregroundStyle(tint)
				.frame(width: 16)

			Text(title)
				.font(.caption)
				.foregroundStyle(.secondary)

			Spacer(minLength: 12)

			Text(value)
				.font(.caption.weight(.medium))
				.lineLimit(1)
				.truncationMode(.middle)
		}
	}
}

struct AccountAvatarView: View {
	let account: CodexAccount
	let title: String

	var body: some View {
		ZStack(alignment: .bottomTrailing) {
			Circle()
				.fill(account.statusColor.opacity(0.14))
				.overlay {
					Circle()
						.strokeBorder(account.statusColor.opacity(0.26), lineWidth: 1)
				}

			Text(AccountDisplay.initials(from: title))
				.font(.caption.weight(.semibold))
				.foregroundStyle(.primary)

			Circle()
				.fill(account.statusColor)
				.frame(width: 8, height: 8)
				.overlay {
					Circle()
						.stroke(.background, lineWidth: 1.5)
				}
		}
		.frame(width: 29, height: 29)
	}
}

struct StatusPillView: View {
	let title: String
	let symbol: String
	let tint: Color

	var body: some View {
		HStack(spacing: 4) {
			Image(systemName: symbol)
			Text(title)
		}
		.font(.caption2.weight(.semibold))
		.foregroundStyle(tint)
		.padding(.horizontal, 7)
		.padding(.vertical, 4)
		.modernGlassSurface(cornerRadius: 9, tint: tint.opacity(0.12))
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

private extension View {
	@ViewBuilder
	func modernGlassSurface(
		cornerRadius: CGFloat,
		tint: Color? = nil,
		interactive: Bool = false
	) -> some View {
		if #available(macOS 26.0, *) {
			self.glassEffect(
				configuredGlass(tint: tint, interactive: interactive),
				in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
			)
		} else {
			self
				.background(
					tint.map { AnyShapeStyle($0) } ?? AnyShapeStyle(.regularMaterial),
					in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
				)
				.overlay {
					RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
						.strokeBorder(Color(nsColor: .separatorColor).opacity(0.28), lineWidth: 0.5)
				}
		}
	}

	@ViewBuilder
	func primaryPanelButtonStyle() -> some View {
		if #available(macOS 26.0, *) {
			self.buttonStyle(.glassProminent)
		} else {
			self.buttonStyle(.borderedProminent)
		}
	}

	@ViewBuilder
	func secondaryPanelButtonStyle() -> some View {
		if #available(macOS 26.0, *) {
			self.buttonStyle(.glass)
		} else {
			self.buttonStyle(.bordered)
		}
	}

	@ViewBuilder
	func iconPanelButtonStyle() -> some View {
		if #available(macOS 26.0, *) {
			self.buttonStyle(.glass)
		} else {
			self.buttonStyle(.borderless)
		}
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
