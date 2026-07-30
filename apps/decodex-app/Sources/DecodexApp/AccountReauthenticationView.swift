import AppKit
import SwiftUI

struct AccountReauthenticationView: View {
	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@State private var copyFeedback = false
	@State private var openFeedback = false

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			if let presentation = store.accountReauthentication {
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

				HStack {
					if let prompt = presentation.prompt {
						Button(openFeedback ? "Opened" : "Open browser") {
							open(prompt.verificationURL)
						}
						.buttonStyle(.borderedProminent)
						.accessibilityHint("Opens the official Codex device sign-in page.")
					}

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
		.frame(width: 256)
		.padding(12)
		.controlSize(.small)
		.interactiveDismissDisabled(true)
	}

	private func header(
		_ presentation: AccountReauthenticationPresentation
	) -> some View {
		VStack(alignment: .leading, spacing: 3) {
			Text("Refresh login")
				.font(PanelFont.transientTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))

			HStack(alignment: .firstTextBaseline, spacing: 4) {
				if presentation.prompt == nil,
					presentation.failureText == nil
				{
					ProgressView()
						.controlSize(.mini)
						.accessibilityHidden(true)
				}

				Text(presentation.accountLabel)
					.font(PanelFont.transientBody)
					.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.86))
					.lineLimit(1)
					.truncationMode(.middle)

				Text("· \(presentation.statusText)")
					.font(PanelFont.tertiary)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.tail)
			}
			.accessibilityElement(children: .combine)
			.accessibilityLabel(
				"\(presentation.accountLabel), \(presentation.statusText)"
			)
		}
	}

	private func promptContent(
		_ prompt: AccountReauthenticationPrompt
	) -> some View {
		VStack(alignment: .leading, spacing: 6) {
			Text("One-time code")
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))

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
			.padding(.vertical, 7)
			.background(
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.fill(
						PanelPalette.secondaryText(colorScheme)
							.opacity(colorScheme == .dark ? 0.12 : 0.08)
					)
			)
			.overlay {
				RoundedRectangle(cornerRadius: 8, style: .continuous)
					.stroke(
						PanelPalette.separator(colorScheme),
						lineWidth: 1
					)
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
