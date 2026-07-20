import SwiftUI

struct AccountRunStripFadeMask: View {
	let metrics: AccountRunStripMetrics

	var body: some View {
		HStack(spacing: 0) {
			if metrics.canScrollBackward {
				LinearGradient(
					colors: [.clear, .black],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: AccountRunStripLayout.fadeWidth)
			}

			Color.black

			if metrics.canScrollForward {
				LinearGradient(
					colors: [.black, .clear],
					startPoint: .leading,
					endPoint: .trailing
				)
				.frame(width: AccountRunStripLayout.fadeWidth)
			}
		}
	}
}

struct AccountRunStripEdgeButton: View {
	let direction: AccountRunStripScrollDirection
	let isEnabled: Bool
	let accessibilityLabel: String
	let disabledHelp: String
	let clickAction: () -> Void
	let startContinuousAction: () -> Void
	let stopContinuousAction: () -> Void
	@Environment(\.colorScheme) private var colorScheme
	@State private var isPressed = false
	@State private var pressTask: Task<Void, Never>?

	init(
		direction: AccountRunStripScrollDirection,
		isEnabled: Bool,
		accessibilityLabel: String? = nil,
		disabledHelp: String? = nil,
		clickAction: @escaping () -> Void,
		startContinuousAction: @escaping () -> Void,
		stopContinuousAction: @escaping () -> Void
	) {
		self.direction = direction
		self.isEnabled = isEnabled
		self.accessibilityLabel = accessibilityLabel ?? direction.accessibilityLabel
		self.disabledHelp = disabledHelp ?? direction.disabledHelp
		self.clickAction = clickAction
		self.startContinuousAction = startContinuousAction
		self.stopContinuousAction = stopContinuousAction
	}

	var body: some View {
		Image(systemName: direction.symbol)
			.font(.system(size: 10.5, weight: .semibold))
			.symbolRenderingMode(.monochrome)
			.foregroundStyle(tint)
			.scaleEffect(isEnabled && isPressed ? 0.92 : 1)
			.frame(
				width: AccountRunStripLayout.edgeControlWidth,
				height: AccountRunChipLayout.height
			)
			.contentShape(Rectangle())
			.allowsHitTesting(isEnabled)
			.highPriorityGesture(
				DragGesture(minimumDistance: 0)
					.onChanged { _ in
						startPress()
					}
					.onEnded { _ in
						endPress()
					}
			)
			.onDisappear {
				cancelPress()
			}
			.onChange(of: isEnabled) { _, isEnabled in
				if isEnabled == false {
					cancelPress()
				}
			}
			.help(isEnabled ? accessibilityLabel : disabledHelp)
			.accessibilityLabel(accessibilityLabel)
			.accessibilityValue(isEnabled ? "Available" : "Unavailable")
	}

	private var tint: Color {
		let opacity: Double
		if isEnabled == false {
			opacity = colorScheme == .dark ? 0.28 : 0.22
		} else if isPressed {
			opacity = 0.92
		} else {
			opacity = colorScheme == .dark ? 0.62 : 0.5
		}

		return PanelPalette.primaryText(colorScheme).opacity(opacity)
	}

	private func startPress() {
		guard isEnabled, pressTask == nil else {
			return
		}

		isPressed = true
		clickAction()
		pressTask = Task {
			try? await Task.sleep(for: AccountRunStripLayout.continuousScrollStartDelay)
			guard Task.isCancelled == false else {
				return
			}

			await MainActor.run {
				startContinuousAction()
			}
		}
	}

	private func endPress() {
		cancelPress()
	}

	private func cancelPress() {
		pressTask?.cancel()
		pressTask = nil
		stopContinuousAction()
		isPressed = false
	}
}
