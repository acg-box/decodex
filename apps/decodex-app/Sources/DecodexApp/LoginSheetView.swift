import AppKit
import SwiftUI

struct LoginSheetView: View {
	let store: AccountStore
	let mode: AccountLoginSheetMode
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
		GlassEffectContainer(spacing: 7) {
			content
		}
	}

	private var content: some View {
		VStack(alignment: .leading, spacing: 7) {
			LoginSheetHeaderView(
				mode: mode,
				statusLabel: store.loginStatusLabel,
				isActive: store.isLoggingIn
					|| store.loginPrompt != nil
					|| store.loginNotice != nil
			)
			LoginCodeCardView(
				code: store.loginPrompt?.compactCode ?? "",
				destinationLabel: loginDestinationLabel,
				canCopy: store.loginPrompt != nil,
				canOpen: store.loginPrompt?.verificationURL != nil,
				copyFeedback: copyFeedback,
				openFeedback: openFeedback,
				copyCode: copyCode,
				openVerificationURL: openVerificationURL
			)

			if isRequestingCode {
				LoginRequestStatusView()
					.transition(.opacity.combined(with: .move(edge: .top)))
			}

			if let notice = store.loginNotice {
				NoticeView(notice: notice, onDismiss: store.clearLoginNotice)
					.id(notice.id)
			}

			LoginSheetActionsView(
				isRequestingCode: isRequestingCode,
				isLoggingIn: store.isLoggingIn,
				requestStarted: requestStarted,
				onCancel: {
					requestStarted = false
					onCancel()
				},
				onPrimary: requestLogin
			)
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
		.onChange(of: store.loginNotice) { _, notice in
			if notice != nil {
				requestStarted = false
			}
		}
	}

	private var isRequestingCode: Bool {
		store.loginPrompt == nil && (requestStarted || store.isLoggingIn)
	}

	private var loginDestinationLabel: String {
		guard let url = store.loginPrompt?.verificationURL else {
			return "chatgpt.com/codex/device"
		}

		return url.host.map { "\($0)\(url.path)" } ?? url.absoluteString
	}

	private func requestLogin() {
		requestStarted = true
		Task {
			let succeeded = await store.login()
			requestStarted = false
			if succeeded {
				onComplete()
			}
		}
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
			try? await Task.sleep(for: .milliseconds(850))
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
			try? await Task.sleep(for: .milliseconds(650))
			guard openFeedbackToken == token else {
				return
			}
			withAnimation(PanelMotion.state) {
				openFeedback = false
			}
		}
	}
}

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
