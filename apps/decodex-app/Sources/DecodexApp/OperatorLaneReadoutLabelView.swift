import SwiftUI

struct OperatorLaneReadoutLabelView: View {
	let title: String
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 3) {
			Image(systemName: symbol)
				.font(.system(size: 8.2, weight: .semibold))
				.foregroundStyle(tint)
				.frame(width: 9.5)

			Text(title)
				.font(OperatorLanePopoverStyle.labelFont)
				.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
				.lineLimit(1)
				.fixedSize(horizontal: true, vertical: false)
		}
		.frame(width: OperatorLaneReadoutLayout.titleWidth, alignment: .leading)
	}

	private var symbol: String {
		switch title.lowercased() {
		case "model", "live model", "model time", "ai runtime", "inference":
			return "waveform"
		case "attempts":
			return "number"
		case "project":
			return "folder"
		case "activity":
			return "waveform.path.ecg"
		case "protocol":
			return "network"
		case "tracker":
			return "clock"
		case "tool", "tools":
			return "hammer"
		case "context":
			return "text.alignleft"
		default:
			return "circle.fill"
		}
	}

	private var tint: Color {
		switch title.lowercased() {
		case "model", "live model", "model time", "ai runtime", "inference":
			return PanelPalette.routeAccent(colorScheme).opacity(0.78)
		case "attempts":
			return PanelPalette.routeAccent(colorScheme).opacity(0.68)
		case "project":
			return PanelPalette.landingAccent(colorScheme).opacity(0.74)
		case "activity":
			return PanelPalette.secondaryText(colorScheme).opacity(0.72)
		case "protocol":
			return PanelPalette.usageCyan(colorScheme).opacity(0.78)
		case "tracker":
			return PanelPalette.secondaryText(colorScheme).opacity(0.68)
		case "tool", "tools":
			return PanelPalette.codexAccent(colorScheme).opacity(0.72)
		case "context":
			return PanelPalette.capacityAccent(colorScheme).opacity(0.72)
		default:
			return PanelPalette.secondaryText(colorScheme).opacity(0.58)
		}
	}
}
