import SwiftUI

struct AccountReauthenticationView: View {
	private enum FocusedAction: Hashable {
		case cancel
	}

	let store: ResetCardStore
	@Environment(\.colorScheme) private var colorScheme
	@FocusState private var focusedAction: FocusedAction?

	var body: some View {
		VStack(alignment: .leading, spacing: 9) {
			if let presentation = store.accountReauthentication {
				let desiredFocus = desiredFocus(for: presentation)

				header(presentation)

				if let failure = presentation.failureText {
					Text(failure)
						.font(PanelFont.transientBody)
						.foregroundStyle(PanelPalette.destructive(colorScheme))
						.fixedSize(horizontal: false, vertical: true)
				}

				HStack(spacing: 8) {
					if presentation.canCloseWithoutCancellation
						|| presentation.canRequestCancellation
					{
						let actionLabel = presentation.canCloseWithoutCancellation
							? "Close login"
							: "Cancel login"
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
					} else {
						ProgressView()
							.controlSize(.mini)
							.frame(width: 38, height: 24)
							.help("Saving login")
							.accessibilityLabel("Saving login")
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
					if presentation.failureText == nil {
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
}
