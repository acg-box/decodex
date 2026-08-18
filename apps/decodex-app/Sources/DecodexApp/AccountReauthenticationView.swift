import AppKit
import SwiftUI

struct AccountReauthenticationView: View {
	private enum FocusedAction: Hashable {
		case continueInBrowser
		case deviceCode
		case cancel
	}

	let store: ResetCardStore
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme
	@FocusState private var focusedAction: FocusedAction?
	@State private var copyFeedback = false
	@State private var openFeedback = false

	var body: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.section) {
			if let presentation = store.accountReauthentication {
				let desiredFocus = desiredFocus(for: presentation)

				header(presentation)

				if presentation.isSelectingMethod {
					methodSelector
						.transition(.panelInline)
				} else if let prompt = presentation.prompt {
					promptContent(prompt)
						.transition(.panelInline)
				}

				if let failure = presentation.failureText {
					Text(failure)
						.font(PanelFont.transientBody)
						.foregroundStyle(PanelPalette.destructive(colorScheme))
						.fixedSize(horizontal: false, vertical: true)
						.transition(.panelInline)
				}

				HStack(spacing: PanelSpacing.section) {
					if presentation.canCloseWithoutCancellation
						|| presentation.canRequestCancellation
					{
						let actionLabel = presentation.isSelectingMethod
							? presentation.cancelActionLabel
							: presentation.canCloseWithoutCancellation
							? presentation.closeActionLabel
							: presentation.cancelActionLabel
						Button {
							if presentation.canCloseWithoutCancellation {
								store.closeAccountReauthentication()
							} else {
								Task {
									await store.cancelAccountReauthentication()
								}
							}
						} label: {
							Image(systemName: "xmark")
								.frame(width: 22, height: 18)
						}
						.buttonStyle(.bordered)
						.keyboardShortcut(.cancelAction)
						.focused($focusedAction, equals: .cancel)
						.help(actionLabel)
						.accessibilityLabel(actionLabel)
						.transition(
							.opacity.combined(
								with: .scale(scale: 0.94, anchor: .leading)
							)
						)
					} else {
						ProgressView()
							.controlSize(.mini)
							.frame(width: 38, height: 24)
							.help(presentation.mode.installingLabel)
							.accessibilityLabel(presentation.mode.installingLabel)
							.transition(
								.opacity.combined(
									with: .scale(scale: 0.9)
								)
							)
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
		.padding(PanelSpacing.popoverInset)
		.controlSize(.small)
		.accessibilityElement(children: .contain)
		.accessibilityLabel(
			store.accountReauthentication?.accessibilityLabel ?? "Account login"
		)
		.animation(
			phaseTransitionAnimation,
			value: store.accountReauthentication?.phase
		)
	}

	private func desiredFocus(
		for presentation: AccountReauthenticationPresentation
	) -> FocusedAction? {
		if presentation.isSelectingMethod {
			return .continueInBrowser
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
		VStack(alignment: .leading, spacing: PanelSpacing.micro) {
			HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.related) {
				Text(presentation.title)
					.font(PanelFont.transientTitle)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))

				Spacer(minLength: 6)

				HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.micro) {
					if presentation.showsProgress {
						ProgressView()
							.controlSize(.mini)
							.transition(.opacity)
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
				presentation.mode == .enrollment
					? presentation.accessibilityLabel
					: "\(presentation.accessibilityLabel) for \(presentation.accountLabel)"
			)

			Text(presentation.statusText)
				.font(PanelFont.transientBody)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.contentTransition(.opacity)
				.lineLimit(1)
				.truncationMode(.tail)
				.accessibilityLabel(presentation.statusText)
		}
	}

	private var methodSelector: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.related) {
			Button("Continue in browser") {
				store.selectAccountLoginMethod(.browserRedirect)
			}
			.buttonStyle(.borderedProminent)
			.keyboardShortcut(.defaultAction)
			.focused($focusedAction, equals: .continueInBrowser)
			.frame(maxWidth: .infinity, alignment: .leading)

			Button("Use device code") {
				store.selectAccountLoginMethod(.deviceCode)
			}
			.buttonStyle(.bordered)
			.focused($focusedAction, equals: .deviceCode)
		}
	}

	private func promptContent(
		_ prompt: AccountReauthenticationPrompt
	) -> some View {
		VStack(alignment: .leading, spacing: PanelSpacing.related) {
			Text(prompt.verificationURL.absoluteString)
				.font(PanelFont.tertiary)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme))
				.textSelection(.enabled)
				.fixedSize(horizontal: false, vertical: true)

			Text(prompt.userCode)
				.font(.system(size: 20, weight: .semibold, design: .monospaced))
				.tracking(1.1)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
				.textSelection(.enabled)
				.accessibilityLabel("One-time login code \(prompt.userCode)")

			HStack(spacing: PanelSpacing.related) {
				Button(copyFeedback ? "Copied" : "Copy code") {
					copy(prompt.userCode)
				}
				.buttonStyle(.bordered)

				Button(openFeedback ? "Opened" : "Open") {
					open(prompt.verificationURL)
				}
				.buttonStyle(.bordered)
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

	private var phaseTransitionAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.controlState
	}
}
