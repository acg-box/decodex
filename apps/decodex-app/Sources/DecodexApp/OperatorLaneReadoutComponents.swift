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

struct OperatorLaneReadoutSummaryView: View {
	let fragments: [[OperatorLaneReadoutTextRun]]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 0) {
			ForEach(fragments.indices, id: \.self) { index in
				OperatorLaneReadoutRunsView(runs: fragments[index])

				if index != fragments.indices.last {
					Text(" · ")
						.font(OperatorLanePopoverStyle.separatorFont)
						.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
				}
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}
}

private struct OperatorLaneReadoutRunsView: View {
	let runs: [OperatorLaneReadoutTextRun]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 0) {
			ForEach(runs.indices, id: \.self) { index in
				let run = runs[index]
				Text(run.text)
					.font(font(for: run.role))
					.foregroundStyle(foreground(for: run.role))
					.monospacedDigit()
					.lineLimit(1)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}

	private func font(for role: OperatorLaneReadoutTextRole) -> Font {
		switch role {
		case .meta:
			return OperatorLanePopoverStyle.metaFont
		case .value:
			return OperatorLanePopoverStyle.valueFont
		}
	}

	private func foreground(for role: OperatorLaneReadoutTextRole) -> Color {
		switch role {
		case .meta:
			return OperatorLanePopoverStyle.mutedText(colorScheme)
		case .value:
			return OperatorLanePopoverStyle.primaryText(colorScheme)
		}
	}
}

struct OperatorLaneReadoutRow: View {
	let title: String
	let items: [OperatorLaneReadoutItem]
	let trailing: String?
	@Environment(\.colorScheme) private var colorScheme

	init(title: String, items: [OperatorLaneReadoutItem], trailing: String? = nil) {
		self.title = title
		self.items = items
		self.trailing = trailing
	}

	var body: some View {
		VStack(alignment: .leading, spacing: OperatorLaneReadoutLayout.itemRowSpacing) {
			HStack(alignment: .firstTextBaseline, spacing: OperatorLaneReadoutLayout.columnSpacing) {
				OperatorLaneReadoutLabelView(title: title)

				if summaryFragments.isEmpty == false {
					OperatorLaneReadoutSummaryView(fragments: summaryFragments)
						.lineLimit(1)
						.allowsTightening(true)
						.fixedSize(horizontal: true, vertical: false)
						.help(accessibilityText)
				} else {
					Spacer(minLength: 0)
				}

				if let trailing = panelTrimmed(trailing) {
					Text(trailing)
						.font(OperatorLanePopoverStyle.metaFont)
						.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
						.lineLimit(1)
						.fixedSize(horizontal: true, vertical: false)
				}
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}

	private var summaryFragments: [[OperatorLaneReadoutTextRun]] {
		let fragments = normalizedTitle == "tools"
			? toolSummaryFragments
			: items.map(\.summaryRuns)
		return fragments.filter { $0.isEmpty == false }
	}

	private var normalizedTitle: String {
		title.lowercased()
	}

	private var toolSummaryFragments: [[OperatorLaneReadoutTextRun]] {
		var fragments = [[OperatorLaneReadoutTextRun]]()
		if let calls = value(for: "tools") ?? value(for: "tool calls") {
			fragments.append([.value(calls), .meta(" tools")])
		}
		if let maxOutput = value(for: "max output") ?? value(for: "max tool output") ?? value(for: "largest output") {
			fragments.append([.value(maxOutput), .meta(" max output")])
		}

		return fragments
	}

	private func value(for label: String) -> String? {
		items.first { $0.matchesLabel(label) }?.displayValue
	}

	private var accessibilityText: String {
		items.map(\.accessibilityText)
			.joined(separator: ", ")
	}
}

struct OperatorTotalMetricsView: View {
	let metrics: [OperatorTotalMetric]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Grid(
			alignment: .leading,
			horizontalSpacing: OperatorLaneReadoutLayout.totalValueLabelSpacing,
			verticalSpacing: OperatorLaneReadoutLayout.itemRowSpacing
		) {
			ForEach(metrics) { metric in
				GridRow(alignment: .firstTextBaseline) {
					OperatorLaneReadoutLabelView(title: metric.title)
						.gridColumnAlignment(.leading)
					ForEach(metricCells(for: metric)) { cell in
						metricCell(cell)
					}
				}
			}
		}
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityLabel(accessibilityText)
	}

	private var accessibilityText: String {
		metrics.map(\.accessibilityText).joined(separator: "; ")
	}

	private func metricCells(for metric: OperatorTotalMetric) -> [OperatorTotalMetricGridCell] {
		(0..<OperatorLaneReadoutLayout.metricColumnCount).flatMap { index in
			let item = metric.items.indices.contains(index) ? metric.items[index] : nil
			let placeholder = item == nil
			return [
				OperatorTotalMetricGridCell(
					id: "\(metric.id)-\(index)-value",
					slot: index,
					text: item?.displayValue ?? "-",
					accessibilityText: item?.accessibilityText,
					role: .value,
					isPlaceholder: placeholder
				),
				OperatorTotalMetricGridCell(
					id: "\(metric.id)-\(index)-label",
					slot: index,
					text: item?.label ?? "-",
					accessibilityText: item?.accessibilityText,
					role: .label,
					isPlaceholder: placeholder
				),
			]
		}
	}

	@ViewBuilder
	private func metricCell(_ cell: OperatorTotalMetricGridCell) -> some View {
		if cell.isPlaceholder {
			Color.clear
				.frame(width: 1, height: 1)
				.gridColumnAlignment(cell.role == .value ? .trailing : .leading)
				.accessibilityHidden(true)
		} else {
			Text(cell.text)
				.font(cell.role == .value ? OperatorLanePopoverStyle.valueFont : OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(cell.role == .value
					? OperatorLanePopoverStyle.primaryText(colorScheme)
					: OperatorLanePopoverStyle.mutedText(colorScheme))
				.monospacedDigit()
				.lineLimit(1)
				.allowsTightening(true)
				.padding(.leading, cell.role == .value && cell.slot > 0
					? OperatorLaneReadoutLayout.totalSegmentSpacing
					: 0)
				.gridColumnAlignment(cell.role == .value ? .trailing : .leading)
				.help(cell.accessibilityText ?? cell.text)
		}
	}
}

private struct OperatorTotalMetricGridCell: Identifiable {
	let id: String
	let slot: Int
	let text: String
	let accessibilityText: String?
	let role: OperatorTotalMetricGridCellRole
	let isPlaceholder: Bool
}

private enum OperatorTotalMetricGridCellRole {
	case value
	case label
}
