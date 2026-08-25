import SwiftUI

struct AccountReauthenticationView: View {
	private enum FocusedAction: Hashable {
		case browser
		case deviceCode
		case codeCard
		case cancel
	}

	let store: ResetCardStore
	@Environment(\.accessibilityReduceMotion) private var reduceMotion
	@Environment(\.colorScheme) private var colorScheme
	@FocusState private var focusedAction: FocusedAction?

	var body: some View {
		VStack(alignment: .leading, spacing: PanelSpacing.section) {
			if let presentation = store.accountReauthentication {
				let desiredFocus = desiredFocus(for: presentation)

				header(presentation)
					.task(id: desiredFocus) {
						await Task.yield()
						guard Task.isCancelled == false else {
							return
						}
						focusedAction = desiredFocus
					}

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
			return .browser
		}
		if presentation.prompt != nil {
			return .codeCard
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
			HStack(alignment: .center, spacing: PanelSpacing.related) {
				HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.micro) {
					Text(presentation.title)
						.font(PanelFont.transientTitle)
						.foregroundStyle(PanelPalette.primaryText(colorScheme))

					if let accountLabel = presentation.headerAccountLabel {
						Text(accountLabel)
							.font(PanelFont.tertiary)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))
							.lineLimit(1)
							.truncationMode(.middle)
					}
				}
				.accessibilityElement(children: .combine)
				.accessibilityLabel(
					presentation.headerAccountLabel.map {
						"\(presentation.accessibilityLabel) for \($0)"
					} ?? presentation.accessibilityLabel
				)

				Spacer(minLength: 6)

				headerAction(presentation)
			}

			if presentation.showsStatusText {
				HStack(alignment: .firstTextBaseline, spacing: PanelSpacing.micro) {
					if presentation.showsProgress
						&& presentation.canRequestCancellation
					{
						ProgressView()
							.controlSize(.mini)
							.transition(.opacity)
							.accessibilityHidden(true)
					}

					Text(presentation.statusText)
						.font(PanelFont.transientBody)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme))
						.lineLimit(1)
						.truncationMode(.tail)
						.contentTransition(.opacity)
						.accessibilityLabel(presentation.statusText)
				}
				.transition(.panelInline)
			}
		}
	}

	@ViewBuilder
	private func headerAction(
		_ presentation: AccountReauthenticationPresentation
	) -> some View {
		if presentation.canCloseWithoutCancellation
			|| presentation.canRequestCancellation
		{
			let actionLabel = presentation.canCloseWithoutCancellation
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
					.font(PanelFont.tertiary)
					.frame(width: 28, height: 28)
					.contentShape(Rectangle())
			}
			.buttonStyle(PanelPressButtonStyle(pressedScale: 0.9))
			.foregroundStyle(PanelPalette.secondaryText(colorScheme))
			.keyboardShortcut(.cancelAction)
			.focused($focusedAction, equals: .cancel)
			.help(actionLabel)
			.accessibilityLabel(actionLabel)
			.transition(
				.opacity.combined(
					with: .scale(scale: 0.94, anchor: .trailing)
				)
			)
		} else {
			ProgressView()
				.controlSize(.mini)
				.frame(width: 28, height: 28)
				.help(presentation.mode.installingLabel)
				.accessibilityLabel(presentation.mode.installingLabel)
				.transition(
					.opacity.combined(
						with: .scale(scale: 0.9)
					)
				)
		}
	}

	private var methodSelector: some View {
		HStack(spacing: PanelSpacing.related) {
			Button {
				store.selectAccountLoginMethod(.browserRedirect)
			} label: {
				Text("Browser")
					.frame(maxWidth: .infinity)
			}
			.buttonStyle(.borderedProminent)
			.keyboardShortcut(.defaultAction)
			.focused($focusedAction, equals: .browser)
			.frame(maxWidth: .infinity)

			Button {
				store.selectAccountLoginMethod(.deviceCode)
			} label: {
				Text("Device code")
					.frame(maxWidth: .infinity)
			}
			.buttonStyle(.bordered)
			.focused($focusedAction, equals: .deviceCode)
			.frame(maxWidth: .infinity)
		}
	}

	private func promptContent(
		_ prompt: AccountReauthenticationPrompt
	) -> some View {
		Button {
			activate(prompt)
		} label: {
			Text(prompt.userCode)
				.font(.system(size: 20, weight: .semibold, design: .monospaced))
				.tracking(1.1)
				.foregroundStyle(PanelPalette.primaryText(colorScheme))
				.frame(maxWidth: .infinity, minHeight: 56)
				.background {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.fill(PanelPalette.actionBlue(colorScheme).opacity(0.12))
				}
				.overlay {
					RoundedRectangle(cornerRadius: 9, style: .continuous)
						.stroke(
							PanelPalette.actionBlue(colorScheme).opacity(0.42),
							lineWidth: 1
						)
				}
		}
		.buttonStyle(PanelPressButtonStyle(pressedScale: 0.98))
		.keyboardShortcut(.defaultAction)
		.focused($focusedAction, equals: .codeCard)
		.contentShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
		.help("Copy the code and open the sign-in page")
		.accessibilityLabel("One-time login code \(prompt.userCode)")
		.accessibilityHint("Activation copies the code and opens the sign-in page")
		.accessibilityAction(named: Text("Copy code and open sign-in page")) {
			activate(prompt)
		}
	}

	private func activate(_ prompt: AccountReauthenticationPrompt) {
		store.activateAccountLoginPrompt(prompt)
	}

	private var phaseTransitionAnimation: Animation? {
		reduceMotion ? nil : PanelMotion.controlState
	}
}
