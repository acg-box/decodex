import AppKit
import SwiftUI

struct AccountReauthenticationView: View {
	private enum FocusedAction: Hashable {
		case cancel
		case openBrowser
	}

	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@FocusState private var focusedAction: FocusedAction?
	@State private var copyFeedback = false
	@State private var openFeedback = false

	var body: some View {
		VStack(alignment: .leading, spacing: 9) {
			if let presentation = store.accountReauthentication {
				let desiredFocus = desiredFocus(for: presentation)

				header(presentation)

				if let prompt = presentation.prompt {
					promptContent(prompt)
				}

				if let failure = presentation.failureText {
					Text(failure)
						.font(PanelFont.transientBody)
						.foregroundStyle(PanelPalette.destructive(colorScheme))
						.fixedSize(horizontal: false, vertical: true)
				}

				HStack(spacing: 8) {
					Button(
						presentation.canCloseWithoutCancellation
							? "Close"
							: presentation.canRequestCancellation ? "Cancel" : "Saving…"
					) {
						if presentation.canCloseWithoutCancellation {
							store.closeAccountReauthentication()
						} else if presentation.canRequestCancellation {
							Task {
								await store.cancelAccountReauthentication()
							}
						}
					}
					.disabled(
						presentation.canCloseWithoutCancellation == false
							&& presentation.canRequestCancellation == false
					)
					.keyboardShortcut(.cancelAction)
					.focused($focusedAction, equals: .cancel)

					Spacer(minLength: 8)

					if let prompt = presentation.prompt {
						Button(openFeedback ? "Opened" : "Open browser") {
							open(prompt.verificationURL)
						}
						.buttonStyle(.borderedProminent)
						.focused($focusedAction, equals: .openBrowser)
						.accessibilityHint("Opens the official Codex device sign-in page.")
					}
				}
				.task(id: desiredFocus) {
					await Task.yield()
					guard Task.isCancelled == false else {
						return
					}
					focusedAction = desiredFocus
				}
			}
		}
		.frame(width: 220)
		.padding(14)
		.controlSize(.small)
		.accessibilityElement(children: .contain)
		.accessibilityLabel("Refresh login")
	}

	private func desiredFocus(
		for presentation: AccountReauthenticationPresentation
	) -> FocusedAction? {
		if presentation.prompt != nil {
			return .openBrowser
		}
		if presentation.canRequestCancellation
			|| presentation.canCloseWithoutCancellation
		{
			return .cancel
		}
		return nil
	}

	private func header(
		_ presentation: AccountReauthenticationPresentation
	) -> some View {
		VStack(alignment: .leading, spacing: 2) {
			HStack(alignment: .firstTextBaseline, spacing: 5) {
				Text("Refresh login")
					.font(PanelFont.transientTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))

				Spacer(minLength: 6)

				HStack(alignment: .firstTextBaseline, spacing: 3) {
					if presentation.prompt == nil,
						presentation.failureText == nil
					{
						ProgressView()
							.controlSize(.mini)
							.accessibilityHidden(true)
					}

					Text(presentation.accountLabel)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.lineLimit(1)
						.truncationMode(.middle)
				}
			}
			.accessibilityElement(children: .combine)
			.accessibilityLabel(
				"Refresh login for \(presentation.accountLabel)"
			)

			Text(presentation.statusText)
				.font(PanelFont.transientBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.tail)
				.accessibilityLabel(presentation.statusText)
		}
	}

	private func promptContent(
		_ prompt: AccountReauthenticationPrompt
	) -> some View {
		VStack(alignment: .leading, spacing: 0) {
			HStack(spacing: 8) {
				Text(prompt.userCode)
					.font(PanelFont.loginCode)
					.monospacedDigit()
					.tracking(1)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
					.textSelection(.enabled)

				Spacer(minLength: 4)

				Button(copyFeedback ? "Copied" : "Copy") {
					copy(prompt.userCode)
				}
				.buttonStyle(.bordered)
				.accessibilityHint("Copies the one-time Codex login code.")
			}
			.padding(.horizontal, 9)
			.padding(.vertical, 8)
			.background {
				RoundedRectangle(cornerRadius: 9, style: .continuous)
					.fill(.ultraThinMaterial)
			}
			.accessibilityElement(children: .contain)
			.accessibilityLabel("One-time login code \(prompt.userCode)")
		}
	}

	private func copy(_ code: String) {
		NSPasteboard.general.clearContents()
		NSPasteboard.general.setString(code, forType: .string)
		copyFeedback = true
		Task { @MainActor in
			try? await Task.sleep(for: .milliseconds(850))
			copyFeedback = false
		}
	}

	private func open(_ url: URL) {
		openFeedback = true
		NSWorkspace.shared.open(url)
		Task { @MainActor in
			try? await Task.sleep(for: .milliseconds(650))
			openFeedback = false
		}
	}
}
