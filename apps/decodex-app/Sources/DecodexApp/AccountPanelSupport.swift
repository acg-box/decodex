import AppKit
import SwiftUI

enum AccountPanelLayout {
	static let accountListScrollSpace = "account-list-scroll"
	static let screenVerticalMargin: CGFloat = 44
	static let panelVerticalPadding: CGFloat = 18
	static let sectionSpacing: CGFloat = 6
	static let headerHeight: CGFloat = 28
	static let accountSummaryHeight: CGFloat = 31
	static let telemetryHorizontalPadding: CGFloat = 7
	static let telemetryTopPadding: CGFloat = 7
	static let telemetryBottomPadding: CGFloat = 2
	static let telemetryVerticalPadding: CGFloat = telemetryTopPadding + telemetryBottomPadding
	static let telemetryRowSpacing: CGFloat = 5
	static let telemetryProfileHeight: CGFloat = 50
	static let telemetryPoolHeight: CGFloat = 16
	static let telemetryPoolMeasuredHeight: CGFloat = 29
	static let noticeHeight: CGFloat = 30
	static let minimumScrollableListHeight: CGFloat = 312

	static func resolvedAccountListContentHeight(
		measured: CGFloat,
		estimated: CGFloat
	) -> CGFloat {
		guard measured.isFinite, measured > 0 else {
			return estimated
		}

		return ceil(measured)
	}

	static func activeScreenVisibleHeight() -> CGFloat {
		let mouseLocation = NSEvent.mouseLocation
		let screen = NSScreen.screens.first { screen in
			screen.frame.contains(mouseLocation)
		} ?? NSScreen.main

		return screen?.visibleFrame.height ?? 760
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
	static let hiddenValue = "hidden"
	static let visibleValue = "visible"
}

struct AccountPanelAnimationKey: Equatable {
	let accountIDs: [String]
	let isInitialLoading: Bool
	let hasAccounts: Bool
	let hasTelemetry: Bool
	let hasNotice: Bool
	let hasUsageProbeError: Bool
	let hasFixedSelection: Bool
	let emailsHidden: Bool
	let needsScrolling: Bool
}
