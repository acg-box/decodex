import SwiftUI

struct OperatorLaneProgressReadoutRow: View {
	let title: String
	let percent: Int
	let elapsed: String
	let total: String
	let barShare: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .center, spacing: OperatorLaneReadoutLayout.columnSpacing) {
			OperatorLaneReadoutLabelView(title: title)

			OperatorLanePopoverProgressBar(progress: barShare)
				.frame(minWidth: OperatorLaneReadoutLayout.progressTrackWidth, maxWidth: .infinity)

			OperatorLaneProgressTextView(percent: percent, elapsed: elapsed, total: total)
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(height: 16)
		.frame(maxWidth: .infinity, alignment: .leading)
	}
}

private struct OperatorLaneProgressTextView: View {
	let percent: Int
	let elapsed: String
	let total: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 0) {
			Text("\(percent)%")
				.font(OperatorLanePopoverStyle.valueFont)
				.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
				.monospacedDigit()

			Text(" · ")
				.font(OperatorLanePopoverStyle.separatorFont)
				.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))

			Text(elapsed)
				.font(OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
				.monospacedDigit()

			Text(" / ")
				.font(OperatorLanePopoverStyle.separatorFont)
				.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))

			Text(total)
				.font(OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
				.monospacedDigit()
		}
		.fixedSize(horizontal: true, vertical: false)
	}
}

private struct OperatorLanePopoverProgressBar: View {
	let progress: CGFloat
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		GeometryReader { proxy in
			let width = max(0, min(1, progress)) * proxy.size.width
			ZStack(alignment: .leading) {
				Capsule()
					.fill(OperatorLanePopoverStyle.progressTrack(colorScheme))
				Capsule()
					.fill(OperatorLanePopoverStyle.progressFill(colorScheme))
					.frame(width: width)
			}
		}
		.frame(height: 3.5)
	}
}

struct OperatorLaneReadoutDivider: View {
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Rectangle()
			.fill(OperatorLanePopoverStyle.separator(colorScheme))
			.frame(height: 0.5)
			.padding(.vertical, 0.5)
	}
}
