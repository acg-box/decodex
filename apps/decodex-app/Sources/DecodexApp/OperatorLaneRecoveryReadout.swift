import SwiftUI

struct OperatorLaneRecoveryReadout: View {
	let recovery: OperatorContinuationRecoveryStatus
	@State private var isExpanded = false
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		VStack(alignment: .leading, spacing: OperatorLaneReadoutLayout.itemRowSpacing) {
			Button {
				isExpanded.toggle()
			} label: {
				HStack(alignment: .firstTextBaseline, spacing: OperatorLaneReadoutLayout.columnSpacing) {
					OperatorLaneReadoutLabelView(title: "Recovery")

					OperatorLaneReadoutSummaryView(fragments: summaryFragments)
						.lineLimit(1)
						.allowsTightening(true)
						.fixedSize(horizontal: true, vertical: false)

					Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
						.font(.system(size: 7.5, weight: .semibold))
						.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
						.frame(width: 8, height: 8)
				}
				.contentShape(Rectangle())
			}
			.buttonStyle(.plain)
			.help(accessibilityText)

			if isExpanded {
				VStack(alignment: .leading, spacing: OperatorLaneReadoutLayout.itemRowSpacing) {
					ForEach(detailItems) { item in
						OperatorLaneRecoveryDetailRow(item: item)
					}
				}
				.padding(.leading, OperatorLaneReadoutLayout.titleWidth + OperatorLaneReadoutLayout.columnSpacing)
				.padding(.top, 1)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityElement(children: .combine)
		.accessibilityLabel(accessibilityText)
	}

	private var summaryFragments: [[OperatorLaneReadoutTextRun]] {
		var fragments = [[OperatorLaneReadoutTextRun]]()
		fragments.append([.value(token(recovery.state, fallback: "continuation"))])
		if recovery.budgetExceeded == true {
			fragments.append([.meta("budget "), .value("exceeded")])
		}

		return fragments
	}

	private var detailItems: [OperatorLaneReadoutItem] {
		[
			OperatorLaneReadoutItem(label: "state", value: token(recovery.state, fallback: "continuation")),
			OperatorLaneReadoutItem(label: "phase", value: phaseValue),
			OperatorLaneReadoutItem(label: "count", value: countValue),
			OperatorLaneReadoutItem(label: "error", value: token(recovery.sourceErrorClass, fallback: "unknown")),
		]
	}

	private var phaseValue: String {
		let phases = [
			tokenOrNil(recovery.sourcePhase),
			tokenOrNil(recovery.nextPhase),
		].compactMap { $0 }

		return phases.isEmpty ? "unknown" : phases.joined(separator: " -> ")
	}

	private var countValue: String {
		let count = recovery.recoveryCount ?? 0
		let limit = recovery.automaticContinuationLimit ?? 0
		let budget = recovery.budgetExceeded == true ? "exceeded" : "within"

		return "\(count)/\(limit) \(budget)"
	}

	private var accessibilityText: String {
		detailItems.map(\.accessibilityText)
			.joined(separator: ", ")
	}

	private func token(_ value: String?, fallback: String) -> String {
		tokenOrNil(value) ?? fallback
	}

	private func tokenOrNil(_ value: String?) -> String? {
		guard let value else {
			return nil
		}

		let token = rawPanelToken(value)

		return token.isEmpty ? nil : token
	}
}

private struct OperatorLaneRecoveryDetailRow: View {
	let item: OperatorLaneReadoutItem
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 6) {
			Text(item.label ?? "value")
				.font(OperatorLanePopoverStyle.metaFont)
				.foregroundStyle(OperatorLanePopoverStyle.mutedText(colorScheme))
				.frame(width: 38, alignment: .leading)

			Text(item.displayValue)
				.font(OperatorLanePopoverStyle.valueFont)
				.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
				.monospacedDigit()
				.lineLimit(1)
				.truncationMode(.middle)
				.frame(maxWidth: 340, alignment: .leading)
				.help(item.accessibilityText)
		}
	}
}
