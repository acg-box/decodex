import Foundation
import SwiftUI

struct AccountUsageMeterView: View {
	let label: String
	let remainingPercent: Int?
	let resetAtUnixEpoch: Int?
	let dailyAveragePercent: Double?
	let tone: AccountTone
	let currentTime: Date
	let refillAnimation: AccountUsageMeterRefillAnimation?
	@Environment(\.colorScheme) var colorScheme
	@Environment(\.accessibilityReduceMotion) private var accessibilityReduceMotion
	@State private var displayedProgress: CGFloat

	init(
		label: String,
		remainingPercent: Int?,
		resetAtUnixEpoch: Int?,
		dailyAveragePercent: Double?,
		tone: AccountTone,
		currentTime: Date,
		refillAnimation: AccountUsageMeterRefillAnimation? = nil
	) {
		self.label = label
		self.remainingPercent = remainingPercent
		self.resetAtUnixEpoch = resetAtUnixEpoch
		self.dailyAveragePercent = dailyAveragePercent
		self.tone = tone
		self.currentTime = currentTime
		self.refillAnimation = refillAnimation
		_displayedProgress = State(
			initialValue: Self.normalizedProgress(
				for: refillAnimation?.fromPercent ?? remainingPercent
			)
		)
	}

	var body: some View {
		VStack(alignment: .leading, spacing: 3) {
			HStack(spacing: 5) {
				Text(label)
					.font(PanelFont.usageLabel)
					.frame(width: 28, alignment: .leading)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme))

				Text(remainingText)
					.font(PanelFont.usageValue)
					.frame(width: 62, alignment: .leading)
					.foregroundStyle(valueColor)
					.monospacedDigit()

				if let dailyAverageText {
					HStack(alignment: .firstTextBaseline, spacing: 3) {
						Text("avg")
							.font(PanelFont.usageLabel)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(0.82))
							.lineLimit(1)

						Text(dailyAverageText)
							.font(PanelFont.usageValue)
							.foregroundStyle(PanelPalette.secondaryText(colorScheme))
							.monospacedDigit()
							.lineLimit(1)
							.minimumScaleFactor(0.78)
					}
					.layoutPriority(1)
				}

				Spacer(minLength: 2)

				Text(resetDisplay.short)
					.font(PanelFont.usageValue)
					.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.82 : 0.9))
					.monospacedDigit()
					.lineLimit(1)

				if resetDisplay.date.isEmpty == false {
					Text(resetDisplay.date)
						.font(PanelFont.tertiary)
						.foregroundStyle(PanelPalette.secondaryText(colorScheme).opacity(colorScheme == .dark ? 0.68 : 0.78))
						.lineLimit(1)
						.truncationMode(.middle)
					}
			}
			.frame(height: 14)

			GeometryReader { proxy in
				ZStack(alignment: .leading) {
					let width = fillWidth(
						in: proxy.size.width,
						progress: displayedProgress
					)

					Capsule()
						.fill(trackColor)
						.overlay {
							Capsule()
								.fill(trackInsetStyle)
								.padding(.vertical, 0.8)
								.allowsHitTesting(false)
						}
					Capsule()
						.fill(fillStyle)
						.frame(width: width)
						.clipShape(Capsule())
						.shadow(
							color: color.opacity(colorScheme == .dark ? 0.09 : 0.07),
							radius: colorScheme == .dark ? 1.2 : 1,
							x: 0,
							y: 0
						)
					Capsule()
						.strokeBorder(trackEdgeColor, lineWidth: 0.24)
						.allowsHitTesting(false)
				}
			}
			.frame(height: 3.2)
		}
		.lineLimit(1)
		.frame(height: 22)
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityText)
		.onChange(of: remainingPercent) { _, current in
			guard refillAnimation == nil else {
				return
			}

			setDisplayedProgressWithoutAnimation(
				Self.normalizedProgress(for: current)
			)
		}
		.onChange(of: accessibilityReduceMotion) { _, reduceMotion in
			guard reduceMotion else {
				return
			}

			setDisplayedProgressWithoutAnimation(progress)
		}
		.task(id: refillAnimation?.id) {
			await runRefillAnimation(refillAnimation)
		}
	}

	@MainActor
	private func runRefillAnimation(
		_ refillAnimation: AccountUsageMeterRefillAnimation?
	) async {
		guard let refillAnimation else {
			setDisplayedProgressWithoutAnimation(progress)
			return
		}
		guard Self.shouldAnimateRefill(refillAnimation, to: remainingPercent) else {
			setDisplayedProgressWithoutAnimation(progress)
			return
		}

		setDisplayedProgressWithoutAnimation(
			Self.normalizedProgress(for: refillAnimation.fromPercent)
		)
		await Task.yield()
		guard Task.isCancelled == false else {
			setDisplayedProgressWithoutAnimation(progress)
			return
		}
		guard accessibilityReduceMotion == false else {
			setDisplayedProgressWithoutAnimation(progress)
			return
		}

		withAnimation(PanelMotion.meterRefill) {
			displayedProgress = progress
		}
	}

	private func setDisplayedProgressWithoutAnimation(_ nextProgress: CGFloat) {
		var transaction = Transaction()
		transaction.disablesAnimations = true
		withTransaction(transaction) {
			displayedProgress = nextProgress
		}
	}
}
