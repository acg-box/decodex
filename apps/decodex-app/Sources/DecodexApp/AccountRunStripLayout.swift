import AppKit
import Foundation

enum AccountRunChipLayout {
	static let height: CGFloat = 18.5
	static let cornerRadius: CGFloat = 9.25
	static let horizontalPadding: CGFloat = 6.5
	static let iconWidth: CGFloat = 9.5
	static let spacing: CGFloat = 4
	static let popoverHoverDelayNanoseconds: UInt64 = 320_000_000
}

enum AccountRunStripLayout {
	static let contentCoordinateSpace = "account-run-strip-content"
	static let dragActivationDistance: CGFloat = 1
	static let edgeControlSpacing: CGFloat = 4
	static let edgeControlWidth: CGFloat = 12
	static let edgeControlReservedWidth = edgeControlWidth * 2 + edgeControlSpacing * 2
	static let fadeWidth: CGFloat = 24
	static let overflowTolerance: CGFloat = 1
	static let wheelLineDeltaScale: CGFloat = 11
	static let wheelMinimumDelta: CGFloat = 0.1
	static let clickScrollDuration: TimeInterval = 0.14
	static let continuousScrollStartDelayNanoseconds: UInt64 = 200_000_000
	static let continuousScrollTickInterval: TimeInterval = 1.0 / 120.0
	static let continuousScrollMaximumFrameInterval: TimeInterval = 1.0 / 20.0
	static let continuousScrollVelocity: CGFloat = 285
}

enum AccountRunStripScrollDirection {
	case backward
	case forward

	var scrollMultiplier: CGFloat {
		switch self {
		case .backward:
			return -1
		case .forward:
			return 1
		}
	}

	var symbol: String {
		switch self {
		case .backward:
			return "chevron.left"
		case .forward:
			return "chevron.right"
		}
	}

	var accessibilityLabel: String {
		switch self {
		case .backward:
			return "Previous running lane"
		case .forward:
			return "Next running lane"
		}
	}

	var disabledHelp: String {
		switch self {
		case .backward:
			return "Already at the first running lane"
		case .forward:
			return "Already at the last running lane"
		}
	}
}

struct AccountRunStripMetrics: Equatable {
	var contentWidth: CGFloat = 0
	var viewportWidth: CGFloat = 0
	var isOverflowing = false
	var canScrollBackward = false
	var canScrollForward = false

	init() {}

	init(contentWidth: CGFloat, viewportWidth: CGFloat, offsetX: CGFloat) {
		self.contentWidth = contentWidth
		self.viewportWidth = viewportWidth
		let maxOffsetX = max(0, contentWidth - viewportWidth)
		isOverflowing = contentWidth > viewportWidth + AccountRunStripLayout.overflowTolerance
		canScrollBackward = isOverflowing && offsetX > 1
		canScrollForward = isOverflowing && offsetX < maxOffsetX - 1
	}
}

final class AccountRunStripPlacementStore {
	private var framesByRunID = [String: CGRect]()

	func update(runID: String, frame: CGRect) {
		framesByRunID[runID] = frame
	}

	func retainOnly(_ runIDs: Set<String>) {
		framesByRunID = framesByRunID.filter { runIDs.contains($0.key) }
	}

	func frame(for runID: String) -> CGRect? {
		framesByRunID[runID]
	}

	func orderedFrames() -> [CGRect] {
		framesByRunID.values.sorted { left, right in
			if left.minX == right.minX {
				return left.width < right.width
			}

			return left.minX < right.minX
		}
	}

	func runID(containing point: NSPoint) -> String? {
		framesByRunID.first { _, frame in
			frame.contains(point)
		}?.key
	}
}
