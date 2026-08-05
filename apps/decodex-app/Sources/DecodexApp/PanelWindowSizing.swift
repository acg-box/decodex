import AppKit
import SwiftUI

private struct PanelWindowSizeReporter: NSViewRepresentable {
	let contentSize: CGSize
	let screenParametersRevision: Int
	let onVisibleFrameChange: (NSRect?) -> Void

	func makeNSView(context: Context) -> NSView {
		let view = PanelWindowSizingProbeView(frame: .zero)
		view.didMoveToWindow = { [weak coordinator = context.coordinator] view in
			PanelWindowAppearance.apply(to: view.window)
			coordinator?.retryResizeAfterWindowAttachment(from: view)
		}
		return view
	}

	func updateNSView(_ nsView: NSView, context: Context) {
		PanelWindowAppearance.apply(to: nsView.window)
		context.coordinator.scheduleResize(
			from: nsView,
			contentSize: contentSize,
			screenParametersRevision: screenParametersRevision,
			onVisibleFrameChange: onVisibleFrameChange
		)
	}

	func makeCoordinator() -> Coordinator {
		Coordinator()
	}

	final class Coordinator {
		private var lastAppliedScreenParametersRevision = -1
		private var pendingSize = CGSize.zero
		private var pendingScreenParametersRevision = 0
		private var pendingVisibleFrameCallback: (NSRect?) -> Void = { _ in }
		private var resizeIsScheduled = false
		private var hasReportedVisibleFrame = false
		private var lastReportedVisibleFrame: NSRect?

		@MainActor
		func scheduleResize(
			from view: NSView,
			contentSize: CGSize,
			screenParametersRevision: Int,
			onVisibleFrameChange: @escaping (NSRect?) -> Void
		) {
			guard contentSize.width > 0, contentSize.height > 0 else {
				return
			}

			pendingSize = contentSize
			pendingScreenParametersRevision = screenParametersRevision
			pendingVisibleFrameCallback = onVisibleFrameChange
			guard resizeIsScheduled == false else {
				return
			}

			resizeIsScheduled = true
			DispatchQueue.main.async { [weak self, weak view] in
				guard let self, let view else {
					return
				}
				self.resizeIsScheduled = false
				self.resizeWindow(
					from: view,
					contentSize: self.pendingSize,
					screenParametersRevision: self.pendingScreenParametersRevision,
					onVisibleFrameChange: self.pendingVisibleFrameCallback
				)
			}
		}

		@MainActor
		private func resizeWindow(
			from view: NSView,
			contentSize: CGSize,
			screenParametersRevision: Int,
			onVisibleFrameChange: (NSRect?) -> Void
		) {
			guard let window = view.window else {
				return
			}

			let visibleFrame = (window.screen ?? NSScreen.main)?.visibleFrame
			reportVisibleFrameIfNeeded(
				visibleFrame,
				onVisibleFrameChange: onVisibleFrameChange
			)
			let targetSize = PanelWindowSizingLayout.roundedContentSize(for: contentSize)
			let frame = PanelWindowSizingLayout.frame(
				forContentSize: targetSize,
				currentFrame: window.frame
			) { contentSize in
				window.frameRect(forContentRect: NSRect(origin: .zero, size: contentSize)).size
			} visibleFrame: {
				visibleFrame
			}
			let screenParametersChanged =
				screenParametersRevision != lastAppliedScreenParametersRevision
			let frameChanged = Self.frameDiffers(frame, window.frame)
			guard screenParametersChanged || frameChanged else {
				return
			}

			lastAppliedScreenParametersRevision = screenParametersRevision
			guard frameChanged else {
				return
			}

			window.setFrame(frame, display: true)
		}

		@MainActor
		func retryResizeAfterWindowAttachment(from view: NSView) {
			guard pendingSize != .zero else {
				return
			}

			scheduleResize(
				from: view,
				contentSize: pendingSize,
				screenParametersRevision: pendingScreenParametersRevision,
				onVisibleFrameChange: pendingVisibleFrameCallback
			)
		}

		@MainActor
		private func reportVisibleFrameIfNeeded(
			_ visibleFrame: NSRect?,
			onVisibleFrameChange: (NSRect?) -> Void
		) {
			guard hasReportedVisibleFrame == false
				|| Self.rectDiffers(visibleFrame, lastReportedVisibleFrame)
			else {
				return
			}

			hasReportedVisibleFrame = true
			lastReportedVisibleFrame = visibleFrame
			onVisibleFrameChange(visibleFrame)
		}

		private static func frameDiffers(_ lhs: NSRect, _ rhs: NSRect) -> Bool {
			abs(lhs.minX - rhs.minX) > 0.5
				|| abs(lhs.minY - rhs.minY) > 0.5
				|| abs(lhs.width - rhs.width) > 0.5
				|| abs(lhs.height - rhs.height) > 0.5
		}

		private static func rectDiffers(_ lhs: NSRect?, _ rhs: NSRect?) -> Bool {
			switch (lhs, rhs) {
			case (.none, .none):
				return false
			case (.none, .some), (.some, .none):
				return true
			case (.some(let lhs), .some(let rhs)):
				return frameDiffers(lhs, rhs)
			}
		}
	}
}

enum PanelWindowAppearance {
	@MainActor
	static func apply(to window: NSWindow?) {
		guard let window else {
			return
		}
		window.hasShadow = false
		window.isOpaque = false
		window.backgroundColor = .clear
		window.contentView?.wantsLayer = true
		window.contentView?.layer?.backgroundColor = NSColor.clear.cgColor
	}
}

private struct PanelWindowSizingModifier: ViewModifier {
	let onVisibleFrameChange: (NSRect?) -> Void
	@State private var contentSize = CGSize.zero
	@State private var screenParametersRevision = 0

	func body(content: Content) -> some View {
		content
			.fixedSize(horizontal: false, vertical: true)
			.background {
				GeometryReader { proxy in
					Color.clear.preference(key: PanelWindowContentSizeKey.self, value: proxy.size)
				}
			}
			.onPreferenceChange(PanelWindowContentSizeKey.self) { size in
				contentSize = size
			}
			.background {
				PanelWindowSizeReporter(
					contentSize: contentSize,
					screenParametersRevision: screenParametersRevision,
					onVisibleFrameChange: onVisibleFrameChange
				)
					.frame(width: 0, height: 0)
			}
			.onReceive(
				NotificationCenter.default.publisher(
					for: NSApplication.didChangeScreenParametersNotification
				)
			) { _ in
				screenParametersRevision &+= 1
			}
			.onReceive(
				NotificationCenter.default.publisher(
					for: NSWindow.didChangeScreenNotification
				)
			) { _ in
				screenParametersRevision &+= 1
			}
	}
}

extension View {
	func sizesPanelWindowToContent(
		onVisibleFrameChange: @escaping (NSRect?) -> Void = { _ in }
	) -> some View {
		modifier(PanelWindowSizingModifier(
			onVisibleFrameChange: onVisibleFrameChange
		))
	}
}

final class PanelWindowSizingProbeView: NSView {
	var didMoveToWindow: ((PanelWindowSizingProbeView) -> Void)?

	override func viewDidMoveToWindow() {
		super.viewDidMoveToWindow()
		didMoveToWindow?(self)
	}
}

struct PanelWindowContentSizeKey: PreferenceKey {
	static let defaultValue = CGSize.zero

	static func reduce(value: inout CGSize, nextValue: () -> CGSize) {
		let next = nextValue()
		guard next != .zero else {
			return
		}
		value = next
	}
}

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
		let requestedFrameSize = frameSizeForContentSize(contentSize)
		guard let visibleFrame = visibleFrame() else {
			return NSRect(
				x: currentFrame.minX,
				y: currentFrame.maxY - requestedFrameSize.height,
				width: requestedFrameSize.width,
				height: requestedFrameSize.height
			)
		}

		let frameSize = fittedFrameSize(
			requestedFrameSize,
			inside: visibleFrame
		)
		let proposedFrame = NSRect(
			// The status-panel controller owns horizontal placement. Keep the
			// current origin while SwiftUI changes only the window size; otherwise a
			// hidden sizing pass can move the panel to the screen edge.
			x: currentFrame.minX,
			y: currentFrame.maxY - frameSize.height,
			width: frameSize.width,
			height: frameSize.height
		)

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

	private static func fittedFrameSize(
		_ frameSize: NSSize,
		inside visibleFrame: NSRect
	) -> NSSize {
		let maximumWidth = max(1, visibleFrame.width - placementMargin * 2)
		let maximumHeight = max(1, visibleFrame.height - placementMargin * 2)

		return NSSize(
			width: min(frameSize.width, maximumWidth),
			height: min(frameSize.height, maximumHeight)
		)
	}

	private static func clamped(_ value: CGFloat, min: CGFloat, max: CGFloat) -> CGFloat {
		guard min <= max else {
			return min
		}

		return Swift.min(Swift.max(value, min), max)
	}
}
