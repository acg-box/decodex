import AppKit
import SwiftUI

enum AccountPanelLayout {
	static let screenVerticalMargin: CGFloat = 44
	static let panelVerticalPadding: CGFloat = 12
	static let panelWidth: CGFloat = 276
	static let minimumAccountListHeight: CGFloat = 110
	static let estimatedAccountRowHeight: CGFloat = 84
	static let statusMaximumHeight: CGFloat = 92
	// Combined header/activity card and panel spacing.
	static let fixedChromeHeight: CGFloat = 90

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
		let contentEstimate = estimatedAccountListContentHeight(
			accountCount: accountCount
		)
		let contentHeight = resolvedAccountListContentHeight(
			measured: measuredContentHeight,
			estimated: contentEstimate
		)

		return min(
			screenBound,
			max(minimumAccountListHeight, contentHeight)
		)
	}

	static func estimatedAccountListContentHeight(
		accountCount: Int
	) -> CGFloat {
		let rowCount = max(1, accountCount)
		let rowHeights = CGFloat(rowCount) * estimatedAccountRowHeight
		let rowGaps =
			CGFloat(max(0, rowCount - 1))
			* PanelSpacing.section
		// The list keeps a one-point rendering inset on every edge so card
		// shadows are not clipped.
		return rowHeights + rowGaps + 2
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

enum AccountCardReorderLayout {
	static let coordinateSpaceName = "decodex.account-reorder-list"

	static func constrainedTranslationY(
		for accountID: String,
		baseOrder: [String],
		frames: [String: CGRect],
		proposed: CGFloat
	) -> CGFloat {
		guard proposed.isFinite,
			let draggedFrame = frames[accountID],
			let firstID = baseOrder.first,
			let firstFrame = frames[firstID],
			let lastID = baseOrder.last,
			let lastFrame = frames[lastID]
		else {
			return 0
		}
		return min(
			max(proposed, firstFrame.minY - draggedFrame.minY),
			lastFrame.maxY - draggedFrame.maxY
		)
	}

	static func reorderedAccountIDs(
		dragging accountID: String,
		baseOrder: [String],
		frames: [String: CGRect],
		translationY: CGFloat
	) -> [String] {
		guard let draggedFrame = frames[accountID],
			baseOrder.contains(accountID),
			baseOrder.allSatisfy({ frames[$0] != nil })
		else {
			return baseOrder
		}

		let draggedCenter = draggedFrame.midY + translationY
		let remaining = baseOrder.filter { $0 != accountID }
		let insertionIndex = remaining.prefix { otherID in
			guard let otherFrame = frames[otherID] else {
				return false
			}
			if translationY > 0 {
				return otherFrame.midY <= draggedCenter
			}
			return otherFrame.midY < draggedCenter
		}.count

		var reordered = remaining
		reordered.insert(accountID, at: insertionIndex)
		return reordered
	}

	static func verticalOffset(
		for accountID: String,
		baseOrder: [String],
		visualOrder: [String],
		frames: [String: CGRect],
		spacing: CGFloat
	) -> CGFloat {
		guard let originalFrame = frames[accountID],
			let projectedFrame = rebasedFrames(
				from: baseOrder,
				to: visualOrder,
				frames: frames,
				spacing: spacing
			)?[accountID]
		else {
			return 0
		}
		return projectedFrame.minY - originalFrame.minY
	}

	static func rebasedFrames(
		from baseOrder: [String],
		to reorderedAccountIDs: [String],
		frames: [String: CGRect],
		spacing: CGFloat
	) -> [String: CGRect]? {
		guard spacing.isFinite,
			baseOrder.count == reorderedAccountIDs.count,
			Set(baseOrder).count == baseOrder.count,
			Set(reorderedAccountIDs) == Set(baseOrder),
			let firstAccountID = baseOrder.first,
			let firstFrame = frames[firstAccountID],
			baseOrder.allSatisfy({ frames[$0] != nil })
		else {
			return nil
		}

		var nextMinY = firstFrame.minY
		var rebasedFrames = [String: CGRect]()
		for accountID in reorderedAccountIDs {
			guard var frame = frames[accountID] else {
				return nil
			}
			frame.origin.y = nextMinY
			rebasedFrames[accountID] = frame
			nextMinY += frame.height + spacing
		}
		return rebasedFrames
	}
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

struct AccountCardFramesPreferenceKey: PreferenceKey {
	static let defaultValue = [String: CGRect]()

	static func reduce(
		value: inout [String: CGRect],
		nextValue: () -> [String: CGRect]
	) {
		value.merge(nextValue()) { _, newFrame in newFrame }
	}
}
