import SwiftUI

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
