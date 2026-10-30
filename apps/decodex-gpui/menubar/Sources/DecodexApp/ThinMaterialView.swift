import AppKit
import SwiftUI

/// Use the complete menu-bar card surface, including its background placement.
private struct ThinMaterialSurface: View {
	let cornerRadius: CGFloat

	var body: some View {
		Color.clear
			.frame(maxWidth: .infinity, maxHeight: .infinity)
			.modifier(PanelCardSurfaceModifier(cornerRadius: cornerRadius))
			.environment(\.panelCardMaterial, .thin)
			.allowsHitTesting(false)
			.accessibilityHidden(true)
	}
}

/// Registered by the existing in-process native library. GPUI retains ownership
/// of its foreground view; SwiftUI renders only the shared Thin material.
@objc(DecodexThinMaterialView)
final class ThinMaterialView: NSView {
	private let background = TransparentHostingView(rootView: ThinMaterialSurface(cornerRadius: 0))

	/// Selection panels can use an exact app-theme tint. Other surfaces keep
	/// the existing menu-bar material and system appearance.
	@objc var tintColor: NSColor? {
		didSet {
			background.isHidden = tintColor != nil
			updateBackdrop()
		}
	}

	@objc var cornerRadius: CGFloat = 0 {
		didSet {
			guard cornerRadius != oldValue else { return }
			background.rootView = ThinMaterialSurface(cornerRadius: cornerRadius)
			updateClip()
		}
	}

	@objc var contentView: NSView? {
		didSet {
			guard contentView !== oldValue else { return }
			oldValue?.removeFromSuperview()
			if let contentView {
				contentView.frame = bounds
				contentView.autoresizingMask = [.width, .height]
				addSubview(contentView, positioned: .above, relativeTo: background)
			}
		}
	}

	override init(frame frameRect: NSRect) {
		super.init(frame: frameRect)
		background.frame = bounds
		background.autoresizingMask = [.width, .height]
		background.sizingOptions = []
		background.safeAreaRegions = []
		background.wantsLayer = true
		background.layer?.backgroundColor = NSColor.clear.cgColor
		addSubview(background)
		wantsLayer = true
		updateBackdrop()
		updateClip()
	}

	required init?(coder: NSCoder) { return nil }
	override var isOpaque: Bool { false }

	override func viewDidChangeEffectiveAppearance() {
		super.viewDidChangeEffectiveAppearance()
		updateBackdrop()
	}

	override func hitTest(_ point: NSPoint) -> NSView? {
		guard let contentView else { return nil }
		return contentView.hitTest(convert(point, from: superview))
	}

	private func updateClip() {
		layer?.cornerRadius = cornerRadius
		layer?.cornerCurve = .continuous
		layer?.masksToBounds = cornerRadius > 0
	}

	private func updateBackdrop() {
		// A window has no card underneath it. Keep Thin's matte backing opaque;
		// otherwise its transparent host still exposes the desktop like glass.
		effectiveAppearance.performAsCurrentDrawingAppearance {
			layer?.backgroundColor = (tintColor ?? .windowBackgroundColor).cgColor
		}
	}
}
