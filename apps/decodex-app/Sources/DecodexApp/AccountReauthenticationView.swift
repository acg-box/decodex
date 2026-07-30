import AppKit
import SwiftUI

struct AccountReauthenticationView: View {
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var copyFeedback = false
	@State private var openFeedback = false

	var body: some View {
		VStack(alignment: .leading, spacing: 10) {
			if let presentation = store.accountReauthentication {
				header(presentation)

				if let prompt = presentation.prompt {
					promptContent(prompt)
				} else if presentation.failureText == nil {
					HStack(spacing: 7) {
						ProgressView()
							.controlSize(.small)
						Text(presentation.statusText)
							.font(PanelFont.transientBody)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					}
				}

				if let failure = presentation.failureText {
					Text(failure)
						.font(PanelFont.transientBody)
						.foregroundStyle(PanelPalette.destructive(colorScheme))
						.fixedSize(horizontal: false, vertical: true)
				}

				Divider()

				HStack {
					Spacer()
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
				}
			}
		}
		.frame(width: 300)
		.padding(14)
		.controlSize(.small)
		.interactiveDismissDisabled(true)
	}

	private func header(
		_ presentation: AccountReauthenticationPresentation
	) -> some View {
		VStack(alignment: .leading, spacing: 3) {
			Text("Refresh Login")
				.font(PanelFont.transientTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))

			Text(presentation.accountLabel)
				.font(PanelFont.transientBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.middle)

			if presentation.prompt != nil {
				Text(presentation.statusText)
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			}
		}
	}

	private func promptContent(
		_ prompt: AccountReauthenticationPrompt
	) -> some View {
		VStack(alignment: .leading, spacing: 8) {
			Text("Enter this one-time code in the Codex sign-in page.")
				.font(PanelFont.transientBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.fixedSize(horizontal: false, vertical: true)

			Text(prompt.userCode)
				.font(.system(size: 24, weight: .semibold, design: .monospaced))
				.monospacedDigit()
				.tracking(1.4)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
				.textSelection(.enabled)
				.frame(maxWidth: .infinity)
				.padding(.vertical, 10)
				.background(
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.fill(
							PanelPalette.secondaryText(colorScheme)
								.opacity(colorScheme == .dark ? 0.12 : 0.08)
						)
				)
				.overlay {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.stroke(
							PanelPalette.separator(colorScheme),
							lineWidth: 1
						)
				}
				.accessibilityLabel("One-time login code \(prompt.userCode)")

			HStack(spacing: 7) {
				Button(copyFeedback ? "Copied" : "Copy Code") {
					copy(prompt.userCode)
				}
				.buttonStyle(.bordered)
				.accessibilityHint("Copies the one-time Codex login code.")

				Button(openFeedback ? "Opened" : "Open Browser") {
					open(prompt.verificationURL)
				}
				.buttonStyle(.borderedProminent)
				.accessibilityHint("Opens the official Codex device sign-in page.")
			}
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
