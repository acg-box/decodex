import AppKit
import SwiftUI

enum AccountPanelLayout {
	static let screenVerticalMargin: CGFloat = 44
	static let panelVerticalPadding: CGFloat = 12
	static let panelWidth: CGFloat = 276
	static let minimumAccountListHeight: CGFloat = 110
	static let estimatedAccountRowHeight: CGFloat = 74
	static let statusMaximumHeight: CGFloat = 92
	// Compact header, aggregate activity card, and panel spacing.
	static let fixedChromeHeight: CGFloat = 114

	static func activeScreenVisibleHeight() -> CGFloat {
		let mouseLocation = NSEvent.mouseLocation
		let screen =
			NSScreen.screens.first { screen in
				screen.frame.contains(mouseLocation)
			} ?? NSScreen.main

		return screen?.visibleFrame.height ?? 760
	}

	static func accountListHeight(
		accountCount: Int,
		measuredContentHeight: CGFloat,
		windowVisibleFrame: CGRect?,
		additionalChromeHeight: CGFloat = 0
	) -> CGFloat {
		let visibleHeight = resolvedScreenVisibleHeight(
			windowVisibleFrame: windowVisibleFrame,
			fallback: activeScreenVisibleHeight()
		)
		let boundedAdditionalChromeHeight =
			additionalChromeHeight.isFinite
			? max(0, additionalChromeHeight)
			: 0
		let screenBound = max(
			minimumAccountListHeight,
			visibleHeight
				- screenVerticalMargin
				- panelVerticalPadding
				- fixedChromeHeight
				- boundedAdditionalChromeHeight
		)
		let rowCount = max(1, accountCount)
		let contentEstimate = CGFloat(rowCount) * estimatedAccountRowHeight
		let contentHeight = resolvedAccountListContentHeight(
			measured: measuredContentHeight,
			estimated: contentEstimate
		)

		return min(
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

enum AccountPrivacy {
	static let hidden = "hidden"
	static let visible = "visible"
}

struct AccountIdentityPresentation: Equatable, Sendable {
	let text: String
	let showsEmail: Bool

	init(alias: String, email: String?, revealsEmail: Bool) {
		let normalizedEmail = email?
			.trimmingCharacters(in: .whitespacesAndNewlines)
		if revealsEmail, let normalizedEmail, normalizedEmail.isEmpty == false {
			text = normalizedEmail
			showsEmail = true
		} else {
			text = alias
			showsEmail = false
		}
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
