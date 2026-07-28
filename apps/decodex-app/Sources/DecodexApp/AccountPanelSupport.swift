import AppKit
import SwiftUI

enum AccountPanelLayout {
	static let screenVerticalMargin: CGFloat = 44
	static let panelVerticalPadding: CGFloat = 18
	static let panelWidth: CGFloat = 344
	static let minimumAccountListHeight: CGFloat = 150
	static let maximumAccountListHeight: CGFloat = 620
	static let estimatedAccountRowHeight: CGFloat = 142
	static let accountRowSpacing: CGFloat = 6
	static let fixedChromeHeight: CGFloat = 116

	static func activeScreenVisibleHeight() -> CGFloat {
		let mouseLocation = NSEvent.mouseLocation
		let screen = NSScreen.screens.first { screen in
			screen.frame.contains(mouseLocation)
		} ?? NSScreen.main

		return screen?.visibleFrame.height ?? 760
	}

	static func accountListHeight(
		accountCount: Int,
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
			+ CGFloat(max(0, rowCount - 1)) * accountRowSpacing

		return min(
			maximumAccountListHeight,
			screenBound,
			max(minimumAccountListHeight, contentEstimate)
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
