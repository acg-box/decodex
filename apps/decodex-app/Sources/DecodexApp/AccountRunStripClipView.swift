import AppKit

final class AccountRunStripClipView: NSClipView {
	var onBoundsChange: (() -> Void)?

	override func constrainBoundsRect(_ proposedBounds: NSRect) -> NSRect {
		var constrainedBounds = super.constrainBoundsRect(proposedBounds)
		constrainedBounds.origin.x = max(0, constrainedBounds.origin.x)
		constrainedBounds.origin.y = max(0, constrainedBounds.origin.y)

		return constrainedBounds
	}

	override func scroll(to newOrigin: NSPoint) {
		let oldOrigin = bounds.origin

		super.scroll(to: newOrigin)
		publishIfNeeded(from: oldOrigin)
	}

	override func setBoundsOrigin(_ newOrigin: NSPoint) {
		let oldOrigin = bounds.origin

		super.setBoundsOrigin(newOrigin)
		publishIfNeeded(from: oldOrigin)
	}

	private func publishIfNeeded(from oldOrigin: NSPoint) {
		guard oldOrigin != bounds.origin else {
			return
		}

		onBoundsChange?()
	}
}
