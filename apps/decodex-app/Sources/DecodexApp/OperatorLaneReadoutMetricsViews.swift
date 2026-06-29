import SwiftUI

struct OperatorTotalMetric: Identifiable {
	let title: String
	let items: [OperatorLaneReadoutItem]

	var id: String {
		title
	}

	var accessibilityText: String {
		let itemText = items.map(\.accessibilityText).joined(separator: ", ")

		return itemText.isEmpty ? title : "\(title), \(itemText)"
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

struct OperatorLifecycleTableRow: Identifiable {
	let lifecycleBucket: String
	let attempts: String
	let runtime: String
	let inputTokens: String
	let outputTokens: String
	let toolCalls: String
	let largestOutput: String

	var id: String {
		lifecycleBucket
	}
}

struct OperatorLifecycleTableView: View {
	let rows: [OperatorLifecycleTableRow]
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		Grid(
			alignment: .leading,
			horizontalSpacing: OperatorLaneReadoutLayout.lifecycleTableColumnSpacing,
			verticalSpacing: OperatorLaneReadoutLayout.itemRowSpacing
		) {
			GridRow(alignment: .firstTextBaseline) {
				headerCell("Lifecycle bucket", alignment: .leading)
				headerCell("attempts", alignment: .trailing)
				headerCell("inference", alignment: .trailing)
				headerCell("input", alignment: .trailing)
				headerCell("output", alignment: .trailing)
				headerCell("tools", alignment: .trailing)
				headerCell("max output", alignment: .trailing)
			}
			ForEach(rows) { row in
				GridRow(alignment: .firstTextBaseline) {
					tableCell(row.lifecycleBucket, alignment: .leading)
					tableCell(row.attempts, alignment: .trailing)
					tableCell(row.runtime, alignment: .trailing)
					tableCell(row.inputTokens, alignment: .trailing)
					tableCell(row.outputTokens, alignment: .trailing)
					tableCell(row.toolCalls, alignment: .trailing)
					tableCell(row.largestOutput, alignment: .trailing)
				}
			}
		}
		.padding(.top, 2)
		.frame(maxWidth: .infinity, alignment: .leading)
		.accessibilityLabel(accessibilityText)
	}

	private var accessibilityText: String {
		rows.map { row in
			"\(row.lifecycleBucket), attempts \(row.attempts), inference \(row.runtime), input \(row.inputTokens), output \(row.outputTokens), tools \(row.toolCalls), max output \(row.largestOutput)"
		}
		.joined(separator: "; ")
	}

	private func headerCell(
		_ text: String,
		alignment: HorizontalAlignment
	) -> some View {
		Text(text)
			.font(OperatorLanePopoverStyle.metaFont)
			.foregroundStyle(OperatorLanePopoverStyle.tableHeaderText(colorScheme))
			.lineLimit(1)
			.allowsTightening(true)
			.gridColumnAlignment(alignment)
	}

	private func tableCell(
		_ text: String,
		alignment: HorizontalAlignment
	) -> some View {
		Text(text)
			.font(OperatorLanePopoverStyle.metaFont)
			.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
			.monospacedDigit()
			.lineLimit(1)
			.allowsTightening(true)
			.gridColumnAlignment(alignment)
	}
}
