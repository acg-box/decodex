import Foundation
import SwiftUI

struct AccountRunChipView: View {
	let card: OperatorCurrentLaneCard
	@Environment(\.colorScheme) private var colorScheme
	@State private var isHovered = false
	@State private var showsPopover = false
	@State private var hoverPopoverTask: Task<Void, Never>?

	var body: some View {
		HStack(spacing: AccountRunChipLayout.spacing) {
			Image(systemName: symbol)
				.font(PanelFont.runChipIcon)
				.foregroundStyle(tint.opacity(colorScheme == .dark ? 0.88 : 0.76))
				.frame(width: AccountRunChipLayout.iconWidth)

			Text(chipTitle)
				.font(PanelFont.runChipTitle)
				.foregroundStyle(PanelPalette.primaryText(colorScheme).opacity(0.92))
				.lineLimit(1)
				.truncationMode(.middle)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: AccountRunChipLayout.height)
		.padding(.horizontal, AccountRunChipLayout.horizontalPadding)
		.background {
			RoundedRectangle(cornerRadius: AccountRunChipLayout.cornerRadius, style: .continuous)
				.fill(isHovered ? tint.opacity(colorScheme == .dark ? 0.09 : 0.07) : Color.clear)
		}
		.modernGlassSurface(cornerRadius: AccountRunChipLayout.cornerRadius, depth: .row)
		.contentShape(RoundedRectangle(cornerRadius: AccountRunChipLayout.cornerRadius, style: .continuous))
		.onHover { hovering in
			isHovered = hovering
			if hovering {
				schedulePopover()
			} else {
				cancelPopover()
			}
		}
		.onDisappear {
			cancelPopover()
		}
		.popover(isPresented: $showsPopover, arrowEdge: .trailing) {
			TimelineView(.periodic(from: Date(), by: 1)) { timeline in
				OperatorLanePopoverView(run: card.run, currentTime: timeline.date)
					.fixedSize(horizontal: true, vertical: false)
			}
		}
	}

	private func schedulePopover() {
		hoverPopoverTask?.cancel()
		hoverPopoverTask = Task {
			try? await Task.sleep(for: AccountRunChipLayout.popoverHoverDelay)
			guard Task.isCancelled == false else {
				return
			}

			await MainActor.run {
				if isHovered {
					showsPopover = true
				}
			}
		}
	}

	private func cancelPopover() {
		hoverPopoverTask?.cancel()
		hoverPopoverTask = nil
		showsPopover = false
	}

	private var symbol: String {
		if card.needsAttention || card.tone == "attention" {
			return "exclamationmark.triangle.fill"
		}
		if card.isWaiting || card.tone == "waiting" {
			return "clock"
		}

		return "play.fill"
	}

	private var chipTitle: String {
		panelTrimmed(card.title) ?? panelTrimmed(card.issueIdentifier) ?? "Run"
	}

	private var tint: Color {
		if card.needsAttention || card.tone == "attention" {
			return PanelPalette.warning(colorScheme)
		}
		if card.isWaiting || card.tone == "waiting" {
			return PanelPalette.secondaryText(colorScheme)
		}

		return PanelPalette.routeAccent(colorScheme)
	}
}

struct AccountRunChipPlacementReporter: ViewModifier {
	let runID: String
	let placementStore: AccountRunStripPlacementStore

	func body(content: Content) -> some View {
		content.background {
			GeometryReader { proxy in
				Color.clear
					.onAppear {
						publish(proxy.frame(in: .named(AccountRunStripLayout.contentCoordinateSpace)))
					}
					.onChange(of: proxy.frame(in: .named(AccountRunStripLayout.contentCoordinateSpace))) { _, frame in
						publish(frame)
					}
			}
		}
	}

	private func publish(_ frame: CGRect) {
		DispatchQueue.main.async {
			placementStore.update(runID: runID, frame: frame)
		}
	}
}
