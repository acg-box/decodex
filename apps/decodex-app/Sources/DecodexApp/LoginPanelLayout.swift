import AppKit

enum LoginPanelLayout {
	static let minimumSize = NSSize(width: 328, height: 190)
	static let fallbackVisibleFrame = NSRect(x: 0, y: 0, width: 1_280, height: 800)
	private static let parentTopOffset: CGFloat = 68
	private static let mouseTopOffset: CGFloat = 18
	private static let placementMargin: CGFloat = 8
	private static let sizeTolerance: CGFloat = 0.5

	static func panelSize(for fittingSize: NSSize) -> NSSize {
		NSSize(
			width: ceil(max(minimumSize.width, fittingSize.width)),
			height: ceil(max(minimumSize.height, fittingSize.height))
		)
	}

	static func origin(
		for panelSize: NSSize,
		parentFrame: NSRect?,
		currentFrame: NSRect?,
		mouseLocation: NSPoint,
		visibleFrame: NSRect
	) -> NSPoint {
		let proposedOrigin: NSPoint
		if let parentFrame {
			proposedOrigin = NSPoint(
				x: parentFrame.midX - panelSize.width / 2,
				y: parentFrame.maxY - panelSize.height - parentTopOffset
			)
		} else if let currentFrame {
			proposedOrigin = NSPoint(
				x: currentFrame.midX - panelSize.width / 2,
				y: currentFrame.maxY - panelSize.height
			)
		} else {
			proposedOrigin = NSPoint(
				x: mouseLocation.x - panelSize.width / 2,
				y: mouseLocation.y - panelSize.height - mouseTopOffset
			)
		}

		return NSPoint(
			x: clamped(
				proposedOrigin.x,
				min: visibleFrame.minX + placementMargin,
				max: visibleFrame.maxX - panelSize.width - placementMargin
			),
			y: clamped(
				proposedOrigin.y,
				min: visibleFrame.minY + placementMargin,
				max: visibleFrame.maxY - panelSize.height - placementMargin
			)
		)
	}

	static func sizeDiffers(_ lhs: NSSize, _ rhs: NSSize) -> Bool {
		abs(lhs.width - rhs.width) > sizeTolerance || abs(lhs.height - rhs.height) > sizeTolerance
	}

	private static func clamped(_ value: CGFloat, min: CGFloat, max: CGFloat) -> CGFloat {
		Swift.min(Swift.max(value, min), max)
	}
}
