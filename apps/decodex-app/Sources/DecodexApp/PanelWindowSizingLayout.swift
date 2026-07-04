import AppKit

enum PanelWindowSizingLayout {
	private static let placementMargin: CGFloat = 8

	static func roundedContentSize(for contentSize: CGSize) -> NSSize {
		NSSize(
			width: ceil(contentSize.width),
			height: ceil(contentSize.height)
		)
	}

	static func frame(
		forContentSize contentSize: NSSize,
		currentFrame: NSRect,
		frameSizeForContentSize: (NSSize) -> NSSize,
		visibleFrame: () -> NSRect?
	) -> NSRect {
		let frameSize = frameSizeForContentSize(contentSize)
		let proposedFrame = NSRect(
			x: currentFrame.midX - frameSize.width / 2,
			y: currentFrame.maxY - frameSize.height,
			width: frameSize.width,
			height: frameSize.height
		)
		guard let visibleFrame = visibleFrame() else {
			return proposedFrame
		}

		return NSRect(
			x: clamped(
				proposedFrame.origin.x,
				min: visibleFrame.minX + placementMargin,
				max: visibleFrame.maxX - frameSize.width - placementMargin
			),
			y: clamped(
				proposedFrame.origin.y,
				min: visibleFrame.minY + placementMargin,
				max: visibleFrame.maxY - frameSize.height - placementMargin
			),
			width: frameSize.width,
			height: frameSize.height
		)
	}

	private static func clamped(_ value: CGFloat, min: CGFloat, max: CGFloat) -> CGFloat {
		guard min <= max else {
			return min
		}

		return Swift.min(Swift.max(value, min), max)
	}
}
