import AppKit
import SwiftUI

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
