import SwiftUI

struct AccountScrollOffsetPreferenceKey: PreferenceKey {
	static let defaultValue: CGFloat = 0

	static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
		value = nextValue()
	}
}

struct AccountListScrollIndicatorView: View {
	let contentHeight: CGFloat
	let viewportHeight: CGFloat
	let scrollOffset: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		if contentHeight > viewportHeight + 1 {
			ZStack(alignment: .top) {
				Capsule(style: .continuous)
					.fill(PanelPalette.secondaryText(colorScheme).opacity(0.12))
					.frame(width: 3, height: viewportHeight)

				Capsule(style: .continuous)
					.fill(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.42 : 0.34))
					.frame(width: 3.5, height: thumbHeight)
					.offset(y: thumbOffset)
			}
			.frame(width: 8, height: viewportHeight)
			.allowsHitTesting(false)
		}
	}

	private var thumbHeight: CGFloat {
		max(30, viewportHeight * min(1, viewportHeight / max(contentHeight, 1)))
	}

	private var thumbOffset: CGFloat {
		let maxScrollOffset = max(1, contentHeight - viewportHeight)
		let maxThumbOffset = max(0, viewportHeight - thumbHeight)
		let progress = min(1, max(0, scrollOffset / maxScrollOffset))

		return maxThumbOffset * progress
	}
}
