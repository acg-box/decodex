import Foundation
import SwiftUI

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

	var summaryRuns: [OperatorLaneReadoutTextRun] {
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

	func matchesLabel(_ expected: String) -> Bool {
		label?.caseInsensitiveCompare(expected) == .orderedSame
	}
}

enum OperatorLaneReadoutTextRole {
	case meta
	case value
}

struct OperatorLaneReadoutTextRun {
	let text: String
	let role: OperatorLaneReadoutTextRole

	static func meta(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .meta)
	}

	static func value(_ text: String) -> OperatorLaneReadoutTextRun {
		OperatorLaneReadoutTextRun(text: text, role: .value)
	}
}

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

func rawPanelToken(_ value: String) -> String {
	value.trimmingCharacters(in: .whitespacesAndNewlines)
}

private struct OperatorLaneReadoutWidthKey: PreferenceKey {
	static let defaultValue: CGFloat = 0

	static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
		value = max(value, nextValue())
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

private struct OperatorLaneReadoutWidthReader: View {
	var body: some View {
		GeometryReader { proxy in
			Color.clear
				.preference(key: OperatorLaneReadoutWidthKey.self, value: proxy.size.width)
		}
	}
}

struct OperatorLanePopoverView: View {
	let run: OperatorRunStatus
	let currentTime: Date
	@State private var readoutWidth: CGFloat = 0

	var body: some View {
		VStack(alignment: .leading, spacing: 4) {
			VStack(alignment: .leading, spacing: 3) {
				if let projectTitle {
					measuredReadout {
						OperatorLaneReadoutRow(
							title: "Project",
							items: [OperatorLaneReadoutItem(label: "project", value: projectTitle)]
						)
					}
				}

				measuredReadout {
					OperatorLaneReadoutRow(
						title: "Activity",
						items: [OperatorLaneReadoutItem(label: nil, value: currentSummary)]
					)
				}

				if statusReadoutItems.isEmpty == false {
					measuredReadout {
						OperatorLaneReadoutRow(title: "Status", items: statusReadoutItems)
					}
				}

				if let modelProgress {
					measuredReadout {
						OperatorLaneProgressReadoutRow(
							title: modelProgress.title,
							percent: modelProgress.percent,
							elapsed: modelProgress.elapsed,
							total: modelProgress.total,
							barShare: modelProgress.barShare
						)
					}
				}

				if let continuationRecovery = run.continuationRecovery {
					measuredReadout {
						OperatorLaneRecoveryReadout(recovery: continuationRecovery)
					}
				}
			}

			if modelProgress != nil,
				totalOverviewMetrics.isEmpty == false
					|| detailBuckets.isEmpty == false
					|| lifecycleTableRows.isEmpty == false
			{
				alignedReadout {
					OperatorLaneReadoutDivider()
				}
			}

			VStack(alignment: .leading, spacing: 3) {
				if totalOverviewMetrics.isEmpty == false {
					measuredReadout {
						OperatorTotalMetricsView(metrics: totalOverviewMetrics)
					}
				}

				ForEach(detailBuckets) { bucket in
					measuredReadout {
						OperatorLaneReadoutRow(
							title: rawPanelToken(bucket.name),
							items: bucketReadoutItems(bucket)
						)
					}
				}

				if lifecycleTableRows.isEmpty == false {
					if totalOverviewMetrics.isEmpty == false || detailBuckets.isEmpty == false {
						alignedReadout {
							OperatorLaneReadoutDivider()
						}
					}
					measuredReadout {
						OperatorLifecycleTableView(rows: lifecycleTableRows)
					}
				}

				if detailBuckets.isEmpty,
					totalOverviewMetrics.isEmpty,
					lifecycleTableRows.isEmpty,
					fallbackRunReadoutItems.isEmpty == false
				{
					measuredReadout {
						OperatorLaneReadoutRow(title: "Run", items: fallbackRunReadoutItems)
					}
				}
			}
		}
		.padding(.horizontal, 10)
		.padding(.vertical, 7)
		.fixedSize(horizontal: true, vertical: false)
		.accessibilityLabel("Lane activity for \(run.compactTitle)")
		.onPreferenceChange(OperatorLaneReadoutWidthKey.self) { width in
			guard abs(width - readoutWidth) > 0.5 else {
				return
			}

			readoutWidth = width
		}
	}

	private var alignedWidth: CGFloat? {
		readoutWidth > 0 ? readoutWidth : nil
	}

	private func measuredReadout<Content: View>(
		@ViewBuilder _ content: () -> Content
	) -> some View {
		content()
			.background(OperatorLaneReadoutWidthReader())
			.frame(width: alignedWidth, alignment: .leading)
	}

	private func alignedReadout<Content: View>(
		@ViewBuilder _ content: () -> Content
	) -> some View {
		content()
			.frame(width: alignedWidth, alignment: .leading)
	}
}

extension OperatorLanePopoverView {
	var activity: OperatorChildAgentActivity? {
		run.childAgentActivity
	}

	var currentSummary: String {
		if run.processAlive == false, run.hasFreshExecution == false {
			if let idle = formatActivityDuration(run.inactiveDurationSeconds) {
				return "Stopped · idle \(idle)"
			}

			return "Stopped"
		}

		guard let activity else {
			return "Waiting for child activity"
		}

		let label = panelTrimmed(activity.currentDetail)
			?? panelTrimmed(activity.currentBucket).map(rawPanelToken)
			?? "Active"
		if let elapsed = formatActivityDuration(activity.currentElapsedSeconds(at: currentTime)) {
			return "\(rawPanelToken(label)) · \(elapsed)"
		}

		return rawPanelToken(label)
	}

	var projectTitle: String? {
		panelTrimmed(run.projectDisplayName) ?? panelTrimmed(run.projectID)
	}

	var statusReadoutItems: [OperatorLaneReadoutItem] {
		var items = [OperatorLaneReadoutItem]()

		if let runPhase = panelTrimmed(run.runPhase ?? run.phase) {
			items.append(
				OperatorLaneReadoutItem(label: "run phase", value: rawPanelToken(runPhase))
			)
		}

		return items
	}

	var fallbackRunReadoutItems: [OperatorLaneReadoutItem] {
		guard let activity else {
			return []
		}

		var items = [
			OperatorLaneReadoutItem(
				label: "wall",
				value: formatActivityDuration(activity.wallSeconds(at: currentTime)) ?? "0s"
			),
			OperatorLaneReadoutItem(
				label: "events",
				value: formatCompactCount(activity.eventCount)
			),
			OperatorLaneReadoutItem(
				label: "input",
				value: "\(formatCompactCount(activity.inputTokensCumulative)) tok"
			),
			OperatorLaneReadoutItem(
				label: "output",
				value: "\(formatCompactCount(activity.outputTokensCumulative)) tok"
			),
			OperatorLaneReadoutItem(
				label: "tools",
				value: formatCompactCount(activity.toolCallCount)
			),
		]

		if let largestOutput = activity.largestToolOutputBytes, largestOutput > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "max output",
					value: formatLargestOutput(bytes: largestOutput)
				)
			)
		}

		return items
	}

	var modelProgress: OperatorModelProgressReadout? {
		operatorModelProgressReadout(for: run, currentTime: currentTime)
	}
}

extension OperatorLanePopoverView {
	var detailBuckets: [OperatorChildAgentBucket] {
		guard run.lifecycleMetrics == nil else {
			return []
		}

		return orderedBuckets.filter { bucket in
			bucket.name.caseInsensitiveCompare("Model") != .orderedSame
				&& detailBucketIsVisible(bucket)
				&& bucketReadoutItems(bucket).isEmpty == false
		}
	}

	var orderedBuckets: [OperatorChildAgentBucket] {
		bucketRows.sorted { left, right in
			let leftPriority = bucketPriority(left.name)
			let rightPriority = bucketPriority(right.name)
			if leftPriority != rightPriority {
				return leftPriority < rightPriority
			}
			let leftWallSeconds = bucketWallSeconds(left)
			let rightWallSeconds = bucketWallSeconds(right)
			if leftWallSeconds != rightWallSeconds {
				return leftWallSeconds > rightWallSeconds
			}
			if left.eventCount != right.eventCount {
				return left.eventCount > right.eventCount
			}

			return left.name < right.name
		}
	}

	var bucketRows: [OperatorChildAgentBucket] {
		activity?.buckets ?? []
	}

	func detailBucketIsVisible(_ bucket: OperatorChildAgentBucket) -> Bool {
		let normalizedName = bucket.name.lowercased()

		return normalizedName.contains("protocol") || normalizedName.contains("tracker")
	}

	func bucketReadoutItems(_ bucket: OperatorChildAgentBucket) -> [OperatorLaneReadoutItem] {
		let normalizedName = bucket.name.lowercased()
		var items = [OperatorLaneReadoutItem]()
		let wallSeconds = bucketWallSeconds(bucket)

		if normalizedName.contains("tracker"), wallSeconds > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "wall",
					value: formatActivityDuration(wallSeconds) ?? "0s"
				)
			)
		}
		if bucket.eventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(bucket.eventCount)))
		}
		if normalizedName.contains("protocol") {
			appendProtocolBucketItems(bucket, to: &items)
		} else {
			appendToolBucketItems(bucket, normalizedName: normalizedName, to: &items)
		}

		return items
	}

	func bucketPriority(_ name: String) -> Int {
		let normalizedName = name.lowercased()
		if normalizedName.contains("model") {
			return 0
		}
		if normalizedName.contains("protocol") {
			return 1
		}
		if normalizedName.contains("tracker") {
			return 2
		}

		return 10
	}

	func bucketWallSeconds(_ bucket: OperatorChildAgentBucket) -> Int {
		activity?.wallSeconds(for: bucket, at: currentTime) ?? bucket.wallSeconds
	}

	func formatLargestOutput(bytes: Int?) -> String {
		guard let bytes, bytes > 0 else {
			return "-"
		}

		return formatCompactBytes(bytes)
	}

	private func appendProtocolBucketItems(
		_ bucket: OperatorChildAgentBucket,
		to items: inout [OperatorLaneReadoutItem]
	) {
		if bucket.inputTokens > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "input",
					value: "\(formatCompactCount(bucket.inputTokens)) tok"
				)
			)
		}
		if bucket.outputTokens > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "output",
					value: "\(formatCompactCount(bucket.outputTokens)) tok"
				)
			)
		}
	}

	private func appendToolBucketItems(
		_ bucket: OperatorChildAgentBucket,
		normalizedName: String,
		to items: inout [OperatorLaneReadoutItem]
	) {
		if bucket.toolCallCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "tools", value: formatCompactCount(bucket.toolCallCount)))
		}
		if bucket.outputBytes > 0 {
			items.append(OperatorLaneReadoutItem(label: "output bytes", value: formatCompactBytes(bucket.outputBytes)))
		}
		guard normalizedName.contains("tracker") == false else {
			return
		}

		appendProtocolBucketItems(bucket, to: &items)
	}
}

extension OperatorLanePopoverView {
	var lifecycleTableRows: [OperatorLifecycleTableRow] {
		guard let lifecycleMetrics = run.lifecycleMetrics else {
			return []
		}

		return lifecycleMetrics.phases.map { phase in
			lifecycleTableRow(
				lifecycleBucket: panelTrimmed(phase.label)
					?? panelTrimmed(phase.phase).map(rawPanelToken)
					?? "Lifecycle bucket",
				attemptCount: phase.attemptCount,
				wallSeconds: phase.wallSeconds,
				buckets: phase.buckets,
				inputTokens: phase.inputTokensCumulative,
				outputTokens: phase.outputTokensCumulative,
				toolCallCount: phase.toolCallCount,
				largestOutputBytes: phase.largestToolOutputBytes
			)
		}
	}

	func lifecycleTableRow(
		lifecycleBucket: String,
		attemptCount: Int,
		wallSeconds: Int,
		buckets: [OperatorLifecycleMetricBucket],
		inputTokens: Int,
		outputTokens: Int,
		toolCallCount: Int,
		largestOutputBytes: Int?
	) -> OperatorLifecycleTableRow {
		let runtimeSeconds = lifecycleModelSeconds(buckets) ?? 0
		let runtime = runtimeShareParts(
			seconds: runtimeSeconds,
			totalSeconds: lifecycleWallSeconds(
				wallSeconds: wallSeconds,
				buckets: buckets,
				runtimeSeconds: runtimeSeconds
			)
		)
		let largestOutput = formatLargestOutput(bytes: largestOutputBytes)

		return OperatorLifecycleTableRow(
			lifecycleBucket: lifecycleBucket,
			attempts: attemptCount > 0 ? formatCompactCount(attemptCount) : "-",
			runtime: runtime.text,
			inputTokens: inputTokens > 0 ? formatCompactCount(inputTokens) : "-",
			outputTokens: outputTokens > 0 ? formatCompactCount(outputTokens) : "-",
			toolCalls: toolCallCount > 0 ? formatCompactCount(toolCallCount) : "-",
			largestOutput: largestOutput
		)
	}

	func lifecycleWallSeconds(
		wallSeconds: Int,
		buckets: [OperatorLifecycleMetricBucket],
		runtimeSeconds: Int
	) -> Int {
		max(
			1,
			wallSeconds,
			buckets.reduce(0) { $0 + max(0, $1.wallSeconds) },
			runtimeSeconds
		)
	}

	func runtimeShareParts(
		seconds: Int,
		totalSeconds: Int
	) -> (percent: String, elapsed: String, total: String, ratio: String, text: String) {
		guard seconds > 0 else {
			return ("-", "-", "-", "-", "-")
		}

		let total = max(1, totalSeconds, seconds)
		let percent = Int((Double(seconds) / Double(total) * 100).rounded())
		let elapsed = formatActivityDuration(seconds) ?? "0s"
		let totalText = formatActivityDuration(total) ?? "0s"
		let compactElapsed = elapsed.replacingOccurrences(of: " ", with: "")
		let compactTotal = totalText.replacingOccurrences(of: " ", with: "")
		let ratio = "\(compactElapsed)/\(compactTotal)"
		return ("\(percent)%", elapsed, totalText, ratio, "\(ratio)(\(percent)%)")
	}

	func lifecycleModelSeconds(_ buckets: [OperatorLifecycleMetricBucket]) -> Int? {
		buckets.first { bucket in
			bucket.name.caseInsensitiveCompare("Model") == .orderedSame
		}?.wallSeconds
	}

	func lifecycleBucket(
		named name: String,
		in buckets: [OperatorLifecycleMetricBucket]
	) -> OperatorLifecycleMetricBucket? {
		buckets.first { bucket in
			bucket.name.caseInsensitiveCompare(name) == .orderedSame
		}
	}
}

extension OperatorLanePopoverView {
	var totalOverviewMetrics: [OperatorTotalMetric] {
		guard let lifecycleMetrics = run.lifecycleMetrics else {
			return []
		}

		return [
			contextMetric(lifecycleMetrics),
			trackerMetric(lifecycleMetrics),
			protocolMetric(lifecycleMetrics),
		].compactMap { $0 }
	}

	func contextMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		var items = [OperatorLaneReadoutItem]()
		if metrics.inputTokensCumulative > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "input",
					value: formatCompactCount(metrics.inputTokensCumulative)
				)
			)
		}
		if metrics.outputTokensCumulative > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "output",
					value: formatCompactCount(metrics.outputTokensCumulative)
				)
			)
		}

		let current = metrics.inputTokensCurrent ?? activity?.inputTokensCurrent
		let peak = metrics.inputTokensPeak ?? activity?.inputTokensMax
		if let current {
			items.append(
				OperatorLaneReadoutItem(
					label: "current window",
					value: formatCompactCount(current)
				)
			)
		}
		if let peak, peak != current {
			items.append(
				OperatorLaneReadoutItem(label: "peak window", value: formatCompactCount(peak))
			)
		}

		guard items.isEmpty == false else {
			return nil
		}

		return OperatorTotalMetric(
			title: "Context",
			items: items
		)
	}

	func trackerMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		guard let bucket = lifecycleBucket(named: "Tracker", in: metrics.buckets) else {
			return nil
		}

		var items = [OperatorLaneReadoutItem]()
		if bucket.eventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(bucket.eventCount)))
		}
		if bucket.toolCallCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "tools", value: formatCompactCount(bucket.toolCallCount)))
		}
		if bucket.outputBytes > 0 {
			items.append(OperatorLaneReadoutItem(label: "output bytes", value: formatCompactBytes(bucket.outputBytes)))
		}

		guard items.isEmpty == false else {
			return nil
		}

		return OperatorTotalMetric(
			title: "Tracker",
			items: items
		)
	}

	func protocolMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		guard metrics.protocolEventCount > 0 || metrics.childEventCount > 0 else {
			return nil
		}

		var items = [OperatorLaneReadoutItem]()
		if metrics.protocolEventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(metrics.protocolEventCount)))
		}
		if metrics.childEventCount > 0 {
			items.append(
				OperatorLaneReadoutItem(
					label: "child events",
					value: formatCompactCount(metrics.childEventCount)
				)
			)
		}

		return OperatorTotalMetric(
			title: "Protocol",
			items: items
		)
	}
}
