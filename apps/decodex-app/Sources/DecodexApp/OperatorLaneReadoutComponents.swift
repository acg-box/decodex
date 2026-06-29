import SwiftUI

struct OperatorLaneHeaderReadoutView: View {
	let status: String
	let project: String?
	@Environment(\.colorScheme) private var colorScheme

	var body: some View {
		HStack(alignment: .firstTextBaseline, spacing: 8) {
			Text(status)
				.font(OperatorLanePopoverStyle.titleFont)
				.foregroundStyle(OperatorLanePopoverStyle.primaryText(colorScheme))
				.lineLimit(1)
				.truncationMode(.tail)
				.fixedSize(horizontal: true, vertical: false)

			if let project = panelTrimmed(project) {
				Text(project)
					.font(OperatorLanePopoverStyle.projectFont)
					.foregroundStyle(OperatorLanePopoverStyle.secondaryText(colorScheme))
					.lineLimit(1)
					.fixedSize(horizontal: true, vertical: false)
					.help(project)
			}
		}
		.fixedSize(horizontal: true, vertical: false)
	}
}

enum OperatorLaneReadoutLayout {
	static let titleWidth: CGFloat = 92
	static let columnSpacing: CGFloat = 7
	static let itemRowSpacing: CGFloat = 2
	static let progressTrackWidth: CGFloat = 84
	static let totalSegmentSpacing: CGFloat = 13
	static let totalValueLabelSpacing: CGFloat = 4
	static let metricColumnCount = 4
	static let lifecycleTableColumnSpacing: CGFloat = 18
}

enum OperatorLanePopoverStyle {
	static let titleFont = PanelFont.laneTitle
	static let projectFont = PanelFont.laneDetail
	static let labelFont = PanelFont.lanePopoverLabel
	static let valueFont = PanelFont.lanePopoverValue
	static let metaFont = PanelFont.lanePopoverMeta
	static let separatorFont = PanelFont.lanePopoverMeta

	static func primaryText(_ colorScheme: ColorScheme) -> Color {
		Color.primary.opacity(colorScheme == .dark ? 0.94 : 0.88)
	}

	static func secondaryText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.92 : 0.86)
	}

	static func mutedText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.78 : 0.68)
	}

	static func tableHeaderText(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.72 : 0.62)
	}

	static func separator(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.18 : 0.22)
	}

	static func progressTrack(_ colorScheme: ColorScheme) -> Color {
		Color.secondary.opacity(colorScheme == .dark ? 0.2 : 0.22)
	}

	static func progressFill(_ colorScheme: ColorScheme) -> Color {
		PanelPalette.routeAccent(colorScheme).opacity(colorScheme == .dark ? 0.86 : 0.76)
	}
}

struct OperatorLaneReadoutItem: Identifiable {
	let label: String?
	let value: String

	init(label: String?, value: String) {
		self.label = label
		self.value = value
	}

	var id: String {
		"\(label ?? "value")-\(value)"
	}

	var displayValue: String {
		if value.hasSuffix(" tok") {
			return String(value.dropLast(4))
		}

		return value
	}

	var accessibilityText: String {
		if let label {
			return "\(label) \(displayValue)"
		}

		return displayValue
	}

	fileprivate var summaryRuns: [OperatorLaneReadoutTextRun] {
		switch label?.lowercased() {
		case "wall":
			return [.meta("wall "), .value(displayValue)]
		case "project":
			return [.meta(displayValue)]
		case "attempts":
			return [.value(displayValue), .meta(displayValue == "1" ? " attempt" : " attempts")]
		case "captured":
			return [.value(displayValue), .meta(" captured")]
		case "missing":
			return [.value(displayValue), .meta(" missing")]
		case "events":
			return [.value(displayValue), .meta(" events")]
		case "child events":
			return [.value(displayValue), .meta(" child events")]
		case "input", "input tokens":
			return [.value(displayValue), .meta(" input")]
		case "output", "output tokens":
			return [.value(displayValue), .meta(" output")]
		case "current":
			return [.value(displayValue), .meta(" current")]
		case "current window", "current window tokens":
			return [.value(displayValue), .meta(" current window")]
		case "peak":
			return [.value(displayValue), .meta(" peak")]
		case "peak window", "peak window tokens":
			return [.value(displayValue), .meta(" peak window")]
		case "tools", "tool calls":
			return [.value(displayValue), .meta(" tools")]
		case "output bytes":
			return [.value(displayValue), .meta(" output")]
		case "max output", "max tool output", "largest output":
			return [.value(displayValue), .meta(" max output")]
		case "largest tool", "source":
			return [.value(displayValue), .meta(" source")]
		default:
			if let label {
				return [.meta("\(label) "), .value(displayValue)]
			}
			return [.value(displayValue)]
		}
	}

	fileprivate var summaryCharacterCount: Int {
		summaryRuns.reduce(0) { total, run in
			total + run.text.count
		}
	}

	func matchesLabel(_ expected: String) -> Bool {
		label?.caseInsensitiveCompare(expected) == .orderedSame
	}
}

fileprivate enum OperatorLaneReadoutTextRole {
	case meta
	case value
}

fileprivate struct OperatorLaneReadoutTextRun {
	let text: String
	let role: OperatorLaneReadoutTextRole

	static func meta(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .meta)
	}

	static func value(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .value)
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

fileprivate struct OperatorLaneRecoveryDetailRow: View {
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

fileprivate struct OperatorLaneReadoutSummaryView: View {
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

fileprivate struct OperatorLaneReadoutRunsView: View {
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
