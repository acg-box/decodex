import AppKit
import SwiftUI

enum AccountLoginSheetMode: Equatable {
	case add
	case reauth(String)

	var title: String {
		switch self {
		case .add:
			return "Device Login"
		case .reauth:
			return "Re-auth Account"
		}
	}

	var icon: String {
		switch self {
		case .add:
			return "person.crop.circle.badge.plus"
		case .reauth:
			return "person.crop.circle.badge.exclamationmark"
		}
	}

	func subtitle(fallback: String, isActive: Bool) -> String {
		switch self {
		case .add:
			return fallback
		case .reauth(let name):
			return !isActive && !name.isEmpty ? name : fallback
		}
	}
}

struct LoginSheetView: View {
	@ObservedObject var store: AccountStore
	let mode: AccountLoginSheetMode
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.dismiss) private var dismiss
	@State private var requestStarted = false

	init(store: AccountStore, mode: AccountLoginSheetMode = .add) {
		self.store = store
		self.mode = mode
	}

	var body: some View {
		VStack(alignment: .leading, spacing: 11) {
			header
			codeCard

			if isRequestingCode {
				requestStatus
					.transition(.opacity.combined(with: .move(edge: .top)))
			}

			if let notice = store.notice {
				Text(notice)
					.font(LoginFont.caption)
					.foregroundStyle(LoginPalette.warning(colorScheme))
					.lineLimit(2)
					.fixedSize(horizontal: false, vertical: true)
			}

			actions
		}
		.frame(width: 322)
		.padding(14)
		.background {
			RoundedRectangle(cornerRadius: 16, style: .continuous)
				.fill(.regularMaterial)
			RoundedRectangle(cornerRadius: 16, style: .continuous)
				.fill(LoginPalette.panelTint(colorScheme))
		}
		.overlay {
			RoundedRectangle(cornerRadius: 16, style: .continuous)
				.strokeBorder(LoginPalette.panelStroke(colorScheme), lineWidth: 0.65)
		}
		.shadow(color: .black.opacity(colorScheme == .dark ? 0.3 : 0.12), radius: 20, y: 10)
		.onChange(of: store.loginPrompt) { _, prompt in
			if prompt != nil {
				requestStarted = false
			}
		}
		.onChange(of: store.notice) { _, notice in
			if notice != nil {
				requestStarted = false
			}
		}
	}

	private var header: some View {
		HStack(spacing: 8) {
			Image(systemName: mode.icon)
				.font(.system(size: 12.5, weight: .semibold))
				.foregroundStyle(LoginPalette.accent(colorScheme))
				.frame(width: 27, height: 27)
				.background {
					RoundedRectangle(cornerRadius: 8, style: .continuous)
						.fill(LoginPalette.controlTint(colorScheme))
				}
				.overlay {
					RoundedRectangle(cornerRadius: 8, style: .continuous)
						.strokeBorder(LoginPalette.controlStroke(colorScheme), lineWidth: 0.55)
				}

			VStack(alignment: .leading, spacing: 1) {
				Text(mode.title)
					.font(LoginFont.title)
					.foregroundStyle(LoginPalette.primaryText(colorScheme))
				Text(
					mode.subtitle(
						fallback: store.loginStatusLabel,
						isActive: store.isLoggingIn || store.loginPrompt != nil || store.notice != nil
					)
				)
					.font(LoginFont.caption)
					.foregroundStyle(LoginPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)
			}

			Spacer()
		}
	}

	private var requestStatus: some View {
		HStack(spacing: 7) {
			ProgressView()
				.controlSize(.small)
				.scaleEffect(0.72)
			Text("Requesting device code")
				.font(LoginFont.caption)
				.foregroundStyle(LoginPalette.secondaryText(colorScheme))
			Spacer(minLength: 0)
		}
		.padding(.horizontal, 9)
		.padding(.vertical, 6)
		.background {
			RoundedRectangle(cornerRadius: 9, style: .continuous)
				.fill(LoginPalette.statusTint(colorScheme))
		}
		.overlay {
			RoundedRectangle(cornerRadius: 9, style: .continuous)
				.strokeBorder(LoginPalette.cardStroke(colorScheme), lineWidth: 0.5)
		}
	}

	private var codeCard: some View {
		VStack(alignment: .leading, spacing: 8) {
			LoginCodeBoxesView(code: store.loginPrompt?.compactCode ?? "")

			HStack(spacing: 6) {
				Text(loginDestinationLabel)
					.font(LoginFont.caption)
					.foregroundStyle(LoginPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)

				Spacer(minLength: 4)

				Button {
					copyCode()
				} label: {
					Image(systemName: "doc.on.doc")
						.frame(width: 22, height: 20)
				}
				.buttonStyle(LoginSmallButtonStyle())
				.disabled(store.loginPrompt == nil)
				.help("Copy code")

				Button {
					openVerificationURL()
				} label: {
					Image(systemName: "arrow.up.forward.app")
						.frame(width: 22, height: 20)
				}
				.buttonStyle(LoginSmallButtonStyle())
				.disabled(store.loginPrompt?.verificationURL == nil)
				.help("Open browser")
			}
		}
		.padding(9)
		.background {
			RoundedRectangle(cornerRadius: 11, style: .continuous)
				.fill(LoginPalette.cardTint(colorScheme))
		}
		.overlay {
			RoundedRectangle(cornerRadius: 11, style: .continuous)
				.strokeBorder(LoginPalette.cardStroke(colorScheme), lineWidth: 0.55)
		}
	}

	private var actions: some View {
		HStack(spacing: 7) {
			Button("Cancel") {
				requestStarted = false
				dismiss()
			}
			.keyboardShortcut(.cancelAction)
			.buttonStyle(LoginTextButtonStyle())

			Spacer()

			Button {
				requestStarted = true
				Task {
					await store.login()
					requestStarted = false
					if store.notice == nil {
						dismiss()
					}
				}
			} label: {
				HStack(spacing: 5) {
					if isRequestingCode {
						ProgressView()
							.controlSize(.small)
							.scaleEffect(0.64)
					} else {
						Image(systemName: store.isLoggingIn ? "clock" : "arrow.right.circle")
					}
					Text(primaryActionTitle)
				}
			}
			.keyboardShortcut(.defaultAction)
			.buttonStyle(LoginTextButtonStyle(isPrimary: true))
			.disabled(store.isLoggingIn || requestStarted)
		}
	}

	private var isRequestingCode: Bool {
		store.loginPrompt == nil && (requestStarted || store.isLoggingIn)
	}

	private var primaryActionTitle: String {
		if isRequestingCode {
			return "Requesting"
		}

		return store.isLoggingIn ? "Waiting" : "Get Code"
	}

	private var loginDestinationLabel: String {
		guard let url = store.loginPrompt?.verificationURL else {
			return "chatgpt.com/codex/device"
		}

		return url.host.map { "\($0)\(url.path)" } ?? url.absoluteString
	}

	private func copyCode() {
		guard let code = store.loginPrompt?.userCode else {
			return
		}

		NSPasteboard.general.clearContents()
		NSPasteboard.general.setString(code, forType: .string)
	}

	private func openVerificationURL() {
		guard let url = store.loginPrompt?.verificationURL else {
			return
		}

		NSWorkspace.shared.open(url)
	}
}

struct FloatingLoginWindowConfigurator: NSViewRepresentable {
	func makeNSView(context: Context) -> NSView {
		let view = NSView()
		configureSoon(from: view)
		return view
	}

	func updateNSView(_ nsView: NSView, context: Context) {
		configureSoon(from: nsView)
	}

	private func configureSoon(from view: NSView) {
		DispatchQueue.main.async {
			guard let window = view.window else {
				return
			}

			window.level = .floating
			window.collectionBehavior.insert(.fullScreenAuxiliary)
			window.isMovableByWindowBackground = true
			window.titleVisibility = .hidden
			window.titlebarAppearsTransparent = true
			window.styleMask.insert(.fullSizeContentView)
		}
	}
}

private struct LoginCodeBoxesView: View {
	let code: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(spacing: 4) {
			ForEach(0..<boxCount, id: \.self) { index in
				if index == 4 {
					Spacer()
						.frame(width: 2)
				}

				Text(character(at: index))
					.font(LoginFont.code)
					.monospacedDigit()
					.foregroundStyle(LoginPalette.primaryText(colorScheme))
					.frame(width: 22, height: 30)
					.background {
						RoundedRectangle(cornerRadius: 7, style: .continuous)
							.fill(LoginPalette.codeCellTint(colorScheme, isEmpty: code.isEmpty))
					}
					.overlay {
						RoundedRectangle(cornerRadius: 7, style: .continuous)
							.strokeBorder(LoginPalette.codeCellStroke(colorScheme), lineWidth: 0.55)
					}
			}
		}
		.frame(maxWidth: .infinity, alignment: .center)
	}

	private var characters: [String] {
		code.map { String($0).uppercased() }
	}

	private var boxCount: Int {
		max(8, min(12, characters.count))
	}

	private func character(at index: Int) -> String {
		guard characters.indices.contains(index) else {
			return ""
		}

		return characters[index]
	}
}

private struct LoginSmallButtonStyle: ButtonStyle {
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(LoginFont.icon)
			.foregroundStyle(LoginPalette.accent(colorScheme).opacity(isEnabled ? 0.9 : 0.34))
			.background {
				RoundedRectangle(cornerRadius: 7, style: .continuous)
					.fill(LoginPalette.controlTint(colorScheme).opacity(configuration.isPressed ? 0.72 : 1))
			}
			.overlay {
				RoundedRectangle(cornerRadius: 7, style: .continuous)
					.strokeBorder(LoginPalette.controlStroke(colorScheme), lineWidth: 0.5)
			}
			.opacity(configuration.isPressed ? 0.78 : 1)
	}
}

private struct LoginTextButtonStyle: ButtonStyle {
	var isPrimary = false
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(LoginFont.button)
			.foregroundStyle(foreground)
			.padding(.horizontal, 11)
			.frame(height: 25)
			.background {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.fill(background.opacity(configuration.isPressed ? 0.72 : 1))
			}
			.overlay {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.strokeBorder(stroke, lineWidth: 0.55)
			}
			.opacity(isEnabled ? 1 : 0.46)
	}

	private var foreground: Color {
		if isPrimary {
			return LoginPalette.accent(colorScheme)
		}

		return LoginPalette.secondaryText(colorScheme)
	}

	private var background: Color {
		isPrimary ? LoginPalette.primaryButtonTint(colorScheme) : LoginPalette.controlTint(colorScheme)
	}

	private var stroke: Color {
		isPrimary ? LoginPalette.primaryButtonStroke(colorScheme) : LoginPalette.controlStroke(colorScheme)
	}
}

private enum LoginFont {
	static let title = Font.system(size: 13.4, weight: .semibold)
	static let caption = Font.system(size: 9.6, weight: .medium)
	static let button = Font.system(size: 10.3, weight: .semibold)
	static let icon = Font.system(size: 10.5, weight: .medium)
	static let code = Font.system(size: 15.8, weight: .semibold, design: .monospaced)
}

private enum LoginPalette {
	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.9, green: 0.94, blue: 0.99).opacity(0.95)
			: Color(red: 0.12, green: 0.17, blue: 0.24).opacity(0.92)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.68, green: 0.76, blue: 0.86).opacity(0.78)
			: Color(red: 0.34, green: 0.43, blue: 0.55).opacity(0.72)
	}

	static func accent(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.72, green: 0.84, blue: 0.98)
			: Color(red: 0.16, green: 0.34, blue: 0.54)
	}

	static func warning(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.88, green: 0.58, blue: 0.35)
			: Color(red: 0.58, green: 0.31, blue: 0.17)
	}

	static func panelTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.13, green: 0.2, blue: 0.3).opacity(0.42)
			: Color(red: 0.72, green: 0.84, blue: 0.95).opacity(0.34)
	}

	static func panelStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.18)
			: Color.white.opacity(0.58)
	}

	static func cardTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.72, green: 0.84, blue: 0.98).opacity(0.095)
			: Color.white.opacity(0.62)
	}

	static func cardStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.13)
			: Color(red: 0.42, green: 0.58, blue: 0.76).opacity(0.2)
	}

	static func statusTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.62, green: 0.78, blue: 1).opacity(0.11)
			: Color(red: 0.86, green: 0.94, blue: 1).opacity(0.62)
	}

	static func controlTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.7, green: 0.8, blue: 0.92).opacity(0.15)
			: Color.white.opacity(0.74)
	}

	static func controlStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.14)
			: Color(red: 0.36, green: 0.52, blue: 0.72).opacity(0.22)
	}

	static func codeCellTint(_ colorScheme: ColorScheme, isEmpty: Bool) -> Color {
		if isEmpty {
			return colorScheme == .dark
				? Color.white.opacity(0.045)
				: Color.white.opacity(0.38)
		}

		return colorScheme == .dark
			? Color(red: 0.72, green: 0.84, blue: 0.98).opacity(0.16)
			: Color(red: 0.9, green: 0.96, blue: 1).opacity(0.84)
	}

	static func codeCellStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color.white.opacity(0.16)
			: Color.white.opacity(0.66)
	}

	static func primaryButtonTint(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.56, green: 0.7, blue: 0.9).opacity(0.18)
			: Color(red: 0.88, green: 0.95, blue: 1).opacity(0.84)
	}

	static func primaryButtonStroke(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.8, green: 0.9, blue: 1).opacity(0.2)
			: Color(red: 0.34, green: 0.54, blue: 0.72).opacity(0.3)
	}
}
