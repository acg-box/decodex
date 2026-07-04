import AppKit

final class PanelWindowSizingProbeView: NSView {
	var didMoveToWindow: ((PanelWindowSizingProbeView) -> Void)?

	override func viewDidMoveToWindow() {
		super.viewDidMoveToWindow()
		didMoveToWindow?(self)
	}
}
