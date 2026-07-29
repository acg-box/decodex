import AppKit
import SwiftUI

enum AccountPanelLayout {
	static let accountListScrollSpace = "account-list-scroll"
	static let screenVerticalMargin: CGFloat = 44
	static let panelVerticalPadding: CGFloat = 18
	static let panelWidth: CGFloat = 322
	static let minimumAccountListHeight: CGFloat = 68
	static let maximumAccountListHeight: CGFloat = 520
	static let estimatedAccountRowHeight: CGFloat = 72
	static let fixedChromeHeight: CGFloat = 96

	static func activeScreenVisibleHeight() -> CGFloat {
		let mouseLocation = NSEvent.mouseLocation
		let screen = NSScreen.screens.first { screen in
			screen.frame.contains(mouseLocation)
		} ?? NSScreen.main

		return screen?.visibleFrame.height ?? 760
	}

	static func accountListHeight(
		accountCount: Int,
		measuredContentHeight: CGFloat,
		windowVisibleFrame: CGRect?,
	) -> CGFloat {
		let visibleHeight = resolvedScreenVisibleHeight(
			windowVisibleFrame: windowVisibleFrame,
			fallback: activeScreenVisibleHeight()
		)
		let screenBound = max(
			minimumAccountListHeight,
			visibleHeight - screenVerticalMargin - panelVerticalPadding - fixedChromeHeight
		)
		let rowCount = max(1, accountCount)
		let contentEstimate = CGFloat(rowCount) * estimatedAccountRowHeight
		let contentHeight = resolvedAccountListContentHeight(
			measured: measuredContentHeight,
			estimated: contentEstimate
		)

		return min(
			maximumAccountListHeight,
			screenBound,
			max(minimumAccountListHeight, contentHeight)
		)
	}

	static func resolvedAccountListContentHeight(
		measured: CGFloat,
		estimated: CGFloat
	) -> CGFloat {
		guard measured.isFinite, measured > 0 else {
			return estimated
		}
		return ceil(measured)
	}

	static func resolvedScreenVisibleHeight(
		windowVisibleFrame: CGRect?,
		fallback: CGFloat
	) -> CGFloat {
		guard let windowVisibleFrame,
			windowVisibleFrame.height.isFinite,
			windowVisibleFrame.height > 0
		else {
			return fallback
		}
		return windowVisibleFrame.height
	}
}

struct AccountScrollOffsetPreferenceKey: PreferenceKey {
	static let defaultValue: CGFloat = 0

	static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
		value = nextValue()
	}
}

struct AccountRowsHeightPreferenceKey: PreferenceKey {
	static let defaultValue: CGFloat = 0

	static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
		let next = nextValue()
		if next > 0 {
			value = next
		}
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
					.fill(
						PanelPalette.secondaryText(colorScheme)
							.opacity(colorScheme == .dark ? 0.42 : 0.34)
					)
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
