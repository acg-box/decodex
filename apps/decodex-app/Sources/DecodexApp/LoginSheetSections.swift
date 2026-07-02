import SwiftUI

struct LoginSheetHeaderView: View {
	let mode: AccountLoginSheetMode
	let statusLabel: String
	let isActive: Bool
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
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
				Text(mode.subtitle(fallback: statusLabel, isActive: isActive))
					.font(LoginFont.caption)
					.foregroundStyle(LoginPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)
			}

			Spacer()
		}
		.padding(.bottom, 1)
	}
}

struct LoginRequestStatusView: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
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
}

struct LoginCodeCardView: View {
	let code: String
	let destinationLabel: String
	let canCopy: Bool
	let canOpen: Bool
	let copyFeedback: Bool
	let openFeedback: Bool
	let copyCode: () -> Void
	let openVerificationURL: () -> Void
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: 8) {
			LoginCodeBoxesView(code: code)

			HStack(spacing: 6) {
				Text(destinationLabel)
					.font(LoginFont.destination)
					.foregroundStyle(LoginPalette.secondaryText(colorScheme))
					.lineLimit(1)
					.truncationMode(.middle)

				Spacer(minLength: 4)

				LoginIconActionButton(
					symbol: "doc.on.doc",
					feedbackSymbol: "checkmark",
					isFeedbackActive: copyFeedback,
					isEnabled: canCopy,
					action: copyCode,
					help: copyFeedback ? "Copied" : "Copy code"
				)

				LoginIconActionButton(
					symbol: "arrow.up.forward.app",
					feedbackSymbol: nil,
					isFeedbackActive: openFeedback,
					isEnabled: canOpen,
					action: openVerificationURL,
					help: openFeedback ? "Opened browser" : "Open browser"
				)
			}
		}
		.padding(8)
		.modernGlassSurface(cornerRadius: 10, depth: .section)
	}
}

struct LoginSheetActionsView: View {
	let isRequestingCode: Bool
	let isLoggingIn: Bool
	let requestStarted: Bool
	let onCancel: () -> Void
	let onPrimary: () -> Void

	var body: some View {
		HStack(spacing: 7) {
			Button("Cancel", action: onCancel)
				.keyboardShortcut(.cancelAction)
				.buttonStyle(LoginTextButtonStyle())

			Spacer()

			Button(action: onPrimary) {
				HStack(spacing: 5) {
					if isRequestingCode {
						ProgressView()
							.controlSize(.small)
							.scaleEffect(0.64)
					} else {
						Image(systemName: isLoggingIn ? "clock" : "arrow.right.circle")
					}
					Text(primaryActionTitle)
				}
			}
			.keyboardShortcut(.defaultAction)
			.buttonStyle(LoginTextButtonStyle(isPrimary: true))
			.disabled(isLoggingIn || requestStarted)
		}
	}

	private var primaryActionTitle: String {
		if isRequestingCode {
			return "Requesting"
		}

		return isLoggingIn ? "Waiting" : "Get Code"
	}
}
