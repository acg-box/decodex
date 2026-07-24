import AppKit
import SwiftUI

struct NoticeView: View {
	let notice: AccountNotice
	var onDismiss: (() -> Void)?
	@Environment(\.colorScheme) private var colorScheme
	@State private var isShowingDetails = false
	@State private var copyFeedback = false
	@State private var copyFeedbackToken = UUID()

	var body: some View {
		HStack(spacing: 6) {
			Image(systemName: symbolName)
				.font(.system(size: 11, weight: .semibold))
				.foregroundStyle(tint)
				.accessibilityHidden(true)

			Text(notice.summary)
				.font(PanelFont.notice)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.9))
				.lineLimit(1)
				.truncationMode(.tail)
				.frame(maxWidth: .infinity, alignment: .leading)

			if notice.details != nil {
				Button("Details") {
					isShowingDetails = true
				}
				.buttonStyle(.plain)
				.font(PanelFont.notice)
				.foregroundStyle(PanelPalette.actionBlue(colorScheme))
				.popover(isPresented: $isShowingDetails, arrowEdge: .trailing) {
					detailsPopover
				}
			}

			if let onDismiss {
				Button(action: onDismiss) {
					Image(systemName: "xmark")
						.font(.system(size: 9, weight: .bold))
						.frame(width: 16, height: 16)
						.contentShape(Rectangle())
				}
				.buttonStyle(.plain)
				.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.72))
				.help("Dismiss")
				.accessibilityLabel("Dismiss notice")
			}
		}
		.padding(.horizontal, 8)
		.frame(height: AccountPanelLayout.noticeHeight)
		.modernGlassSurface(
			cornerRadius: 8,
			depth: .section
		)
	}

	private var detailsPopover: some View {
		VStack(alignment: .leading, spacing: 9) {
			HStack(spacing: 7) {
				Image(systemName: symbolName)
					.foregroundStyle(tint)
					.accessibilityHidden(true)
				Text(notice.summary)
					.font(PanelFont.accountName)
					.foregroundStyle(PanelPalette.primaryText(colorScheme))
			}

			ScrollView {
				Text(notice.copyText)
					.font(PanelFont.notice)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))
					.textSelection(.enabled)
					.frame(maxWidth: .infinity, alignment: .leading)
			}
			.frame(maxHeight: 150)

			HStack {
				Spacer()
				Button(copyFeedback ? "Copied" : "Copy details") {
					copyDetails()
				}
				.buttonStyle(.bordered)
				.controlSize(.small)
			}
		}
		.padding(12)
		.frame(width: 280)
	}

	private var symbolName: String {
		switch notice.tone {
		case .success:
			return "checkmark.circle.fill"
		case .information:
			return "info.circle.fill"
		case .error:
			return "exclamationmark.triangle.fill"
		}
	}

	private var tint: Color {
		switch notice.tone {
		case .success:
			return PanelPalette.capacityAccent(colorScheme)
		case .information:
			return PanelPalette.secondaryText(colorScheme)
		case .error:
			return PanelPalette.warning(colorScheme)
		}
	}

	private func copyDetails() {
		NSPasteboard.general.clearContents()
		NSPasteboard.general.setString(notice.copyText, forType: .string)

		let token = UUID()
		copyFeedbackToken = token
		withAnimation(PanelMotion.state) {
			copyFeedback = true
		}

		Task { @MainActor in
			try? await Task.sleep(for: .milliseconds(900))
			guard copyFeedbackToken == token else {
				return
			}
			withAnimation(PanelMotion.state) {
				copyFeedback = false
			}
		}
	}
}
