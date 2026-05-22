import AppKit
import SwiftUI

enum AccountLoginSheetMode: Equatable {
	case newAccount
	case account(String)

	var title: String {
		"Login"
	}

	var icon: String {
		"person.crop.circle.badge.plus"
	}

	func subtitle(fallback: String, isActive: Bool) -> String {
		switch self {
		case .newAccount:
			return fallback
		case .account(let name):
			return isActive == false && name.isEmpty == false ? name : fallback
		}
	}
}

struct LoginSheetView: View {
	@ObservedObject var store: AccountStore
	let mode: AccountLoginSheetMode
	@Environment(\.colorScheme) private var colorScheme
	@State private var requestStarted = false
	@State private var copyFeedback = false
	@State private var copyFeedbackToken = UUID()
	@State private var openFeedback = false
	@State private var openFeedbackToken = UUID()
	private let onCancel: () -> Void
	private let onComplete: () -> Void

	init(
		store: AccountStore,
		mode: AccountLoginSheetMode = .newAccount,
		onCancel: @escaping () -> Void = {},
		onComplete: @escaping () -> Void = {}
	) {
		self.store = store
		self.mode = mode
		self.onCancel = onCancel
		self.onComplete = onComplete
	}

	var body: some View {
		Group {
			if #available(macOS 26.0, *) {
				GlassEffectContainer(spacing: 7) {
					content
				}
			} else {
				content
			}
		}
	}

	private var content: some View {
		VStack(alignment: .leading, spacing: 7) {
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
		.frame(width: 310)
		.padding(9)
		.modernGlassSurface(cornerRadius: 18, depth: .panel)
		.controlSize(.small)
		.symbolRenderingMode(.hierarchical)
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
				.font(LoginFont.icon)
				.foregroundStyle(LoginPalette.accent(colorScheme))
				.frame(width: 28, height: 28)
				.modernGlassSurface(cornerRadius: 9, depth: .control)

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
		.padding(.bottom, 1)
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
		.padding(.horizontal, 8)
		.padding(.vertical, 5)
		.modernGlassSurface(cornerRadius: 9, depth: .section)
	}

	private var codeCard: some View {
		VStack(alignment: .leading, spacing: 8) {
			LoginCodeBoxesView(code: store.loginPrompt?.compactCode ?? "")

			HStack(spacing: 6) {
				Text(loginDestinationLabel)
					.font(LoginFont.destination)
					.foregroundStyle(LoginPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)

				Spacer(minLength: 4)

				LoginIconActionButton(
					symbol: "doc.on.doc",
					feedbackSymbol: "checkmark",
					isFeedbackActive: copyFeedback,
					isEnabled: store.loginPrompt != nil,
					action: copyCode,
					help: copyFeedback ? "Copied" : "Copy code"
				)

				LoginIconActionButton(
					symbol: "arrow.up.forward.app",
					feedbackSymbol: nil,
					isFeedbackActive: openFeedback,
					isEnabled: store.loginPrompt?.verificationURL != nil,
					action: openVerificationURL,
					help: openFeedback ? "Opened browser" : "Open browser"
				)
			}
		}
		.padding(8)
		.modernGlassSurface(cornerRadius: 10, depth: .section)
	}

	private var actions: some View {
		HStack(spacing: 7) {
			Button("Cancel") {
				requestStarted = false
				onCancel()
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
						onComplete()
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
		showCopyFeedback()
	}

	private func openVerificationURL() {
		guard let url = store.loginPrompt?.verificationURL else {
			return
		}

		showOpenFeedback()
		NSWorkspace.shared.open(url)
	}

	private func showCopyFeedback() {
		let token = UUID()
		copyFeedbackToken = token
		withAnimation(PanelMotion.state) {
			copyFeedback = true
		}

		Task { @MainActor in
			try? await Task.sleep(nanoseconds: 850_000_000)
			guard copyFeedbackToken == token else {
				return
			}
			withAnimation(PanelMotion.state) {
				copyFeedback = false
			}
		}
	}

	private func showOpenFeedback() {
		let token = UUID()
		openFeedbackToken = token
		withAnimation(PanelMotion.state) {
			openFeedback = true
		}

		Task { @MainActor in
			try? await Task.sleep(nanoseconds: 650_000_000)
			guard openFeedbackToken == token else {
				return
			}
			withAnimation(PanelMotion.state) {
				openFeedback = false
			}
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
					.modernGlassSurface(cornerRadius: 7, depth: .control)
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

private struct LoginIconActionButton: View {
	let symbol: String
	let feedbackSymbol: String?
	let isFeedbackActive: Bool
	let isEnabled: Bool
	let action: () -> Void
	let help: String
	@State private var isHovered = false

	var body: some View {
		Button(action: action) {
			Image(systemName: isFeedbackActive ? (feedbackSymbol ?? symbol) : symbol)
				.frame(width: 23, height: 22)
		}
		.buttonStyle(
			LoginSmallButtonStyle(
				isProminent: isHovered,
				isFeedbackActive: isFeedbackActive
			)
		)
		.disabled(isEnabled == false)
		.onHover { hovering in
			withAnimation(PanelMotion.hover) {
				isHovered = hovering
			}
		}
		.help(help)
	}
}

private struct LoginSmallButtonStyle: ButtonStyle {
	var isProminent = false
	var isFeedbackActive = false
	@Environment(\.colorScheme) private var colorScheme
	@Environment(\.isEnabled) private var isEnabled

	func makeBody(configuration: Configuration) -> some View {
		configuration.label
			.font(LoginFont.icon)
			.foregroundStyle(foreground)
			.modernGlassSurface(cornerRadius: 8, depth: .control)
			.overlay {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.strokeBorder(stroke, lineWidth: 0.55)
					.allowsHitTesting(false)
			}
			.shadow(
				color: shadowColor,
				radius: isFeedbackActive ? 4 : (isProminent ? 2.4 : 0),
				y: isFeedbackActive ? 1.2 : (isProminent ? 0.8 : 0)
			)
			.scaleEffect(configuration.isPressed ? 0.91 : (isFeedbackActive ? 1.055 : 1))
			.opacity(isEnabled ? 1 : 0.38)
			.animation(PanelMotion.press, value: configuration.isPressed)
			.animation(PanelMotion.hover, value: isProminent)
			.animation(PanelMotion.state, value: isFeedbackActive)
	}

	private var foreground: Color {
		if isEnabled == false {
			return LoginPalette.secondaryText(colorScheme).opacity(0.62)
		}
		if isFeedbackActive {
			return LoginPalette.feedback(colorScheme)
		}
		if isProminent {
			return LoginPalette.accent(colorScheme).opacity(colorScheme == .dark ? 1 : 0.95)
		}
		return LoginPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.9 : 0.78)
	}

	private var stroke: Color {
		if isEnabled == false {
			return LoginPalette.secondaryText(colorScheme).opacity(0.08)
		}
		if isFeedbackActive {
			return LoginPalette.feedback(colorScheme).opacity(colorScheme == .dark ? 0.36 : 0.26)
		}
		if isProminent {
			return LoginPalette.accent(colorScheme).opacity(colorScheme == .dark ? 0.22 : 0.18)
		}
		return LoginPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.1 : 0.12)
	}

	private var shadowColor: Color {
		if isFeedbackActive {
			return LoginPalette.feedback(colorScheme).opacity(colorScheme == .dark ? 0.22 : 0.16)
		}
		if isProminent {
			return Color.black.opacity(colorScheme == .dark ? 0.22 : 0.1)
		}
		return .clear
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
			.padding(.horizontal, isPrimary ? 11 : 9)
			.frame(height: 24)
			.modernGlassSurface(cornerRadius: 12, depth: .control)
			.scaleEffect(configuration.isPressed ? 0.965 : 1)
			.opacity(isEnabled ? 1 : 0.64)
			.animation(.interactiveSpring(response: 0.16, dampingFraction: 0.78), value: configuration.isPressed)
	}

	private var foreground: Color {
		if isPrimary {
			return LoginPalette.accent(colorScheme)
		}

		return LoginPalette.secondaryText(colorScheme)
	}

}

private enum LoginPalette {
	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.98, green: 0.985, blue: 1).opacity(0.99)
			: Color(red: 0.09, green: 0.11, blue: 0.15).opacity(0.96)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.86, green: 0.89, blue: 0.95).opacity(0.94)
			: Color(red: 0.28, green: 0.33, blue: 0.4).opacity(0.86)
	}

	static func accent(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.actionBlue(colorScheme)
	}

	static func warning(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.warning(colorScheme)
	}

	static func feedback(_ colorScheme: ColorScheme) -> Color {
		colorScheme == .dark
			? Color(red: 0.86, green: 0.93, blue: 1)
			: Color(red: 0.13, green: 0.32, blue: 0.52)
	}
}
