import AppKit

final class LoginPanelPlacementState {
	weak var lastParent: NSWindow?
	var lastPanelSize: NSSize?
	var hasPlacedPanel = false

	func reset() {
		lastParent = nil
		lastPanelSize = nil
		hasPlacedPanel = false
	}

	func shouldPlace(
		parentWindow: NSWindow?,
		panelSize: NSSize,
		forcePlacement: Bool
	) -> Bool {
		let parentChanged = Self.windowsDiffer(lastParent, parentWindow)
		let sizeChanged = lastPanelSize.map {
			LoginPanelLayout.sizeDiffers($0, panelSize)
		} ?? true

		return forcePlacement || parentChanged || sizeChanged
	}

	func markPlaced(parentWindow: NSWindow?, panelSize: NSSize) {
		lastParent = parentWindow
		lastPanelSize = panelSize
		hasPlacedPanel = true
	}

	func parentDiffers(from window: NSWindow?) -> Bool {
		Self.windowsDiffer(lastParent, window)
	}

	private static func windowsDiffer(_ lhs: NSWindow?, _ rhs: NSWindow?) -> Bool {
		switch (lhs, rhs) {
		case (nil, nil):
			return false
		case (.some(let lhs), .some(let rhs)):
			return lhs !== rhs
		default:
			return true
		}
	}
}
