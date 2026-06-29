import Foundation
import SwiftUI

private struct OperatorLaneReadoutWidthKey: PreferenceKey {
	static let defaultValue: CGFloat = 0

	static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
		value = max(value, nextValue())
	}
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
						OperatorLaneReadoutRow(title: rawPanelToken(bucket.name), items: bucketReadoutItems(bucket))
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

	private var activity: OperatorChildAgentActivity? {
		run.childAgentActivity
	}

	private var currentSummary: String {
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

	private var projectTitle: String? {
		panelTrimmed(run.projectDisplayName) ?? panelTrimmed(run.projectID)
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

	private var hasReadoutContent: Bool {
		modelProgress != nil
			|| statusReadoutItems.isEmpty == false
			|| totalOverviewMetrics.isEmpty == false
			|| detailBuckets.isEmpty == false
			|| lifecycleTableRows.isEmpty == false
			|| fallbackRunReadoutItems.isEmpty == false
	}

	private var statusReadoutItems: [OperatorLaneReadoutItem] {
		var items = [OperatorLaneReadoutItem]()

		if let runPhase = panelTrimmed(run.runPhase ?? run.phase) {
			items.append(
				OperatorLaneReadoutItem(label: "run phase", value: rawPanelToken(runPhase))
			)
		}

		return items
	}

	private var fallbackRunReadoutItems: [OperatorLaneReadoutItem] {
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

	private var modelProgress: OperatorModelProgressReadout? {
		operatorModelProgressReadout(for: run, currentTime: currentTime)
	}

	private var detailBuckets: [OperatorChildAgentBucket] {
		guard run.lifecycleMetrics == nil else {
			return []
		}

		return orderedBuckets.filter { bucket in
			bucket.name.caseInsensitiveCompare("Model") != .orderedSame
				&& detailBucketIsVisible(bucket)
				&& bucketReadoutItems(bucket).isEmpty == false
		}
	}

	private func detailBucketIsVisible(_ bucket: OperatorChildAgentBucket) -> Bool {
		let normalizedName = bucket.name.lowercased()

		return normalizedName.contains("protocol") || normalizedName.contains("tracker")
	}

	private var orderedBuckets: [OperatorChildAgentBucket] {
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

	private var totalOverviewMetrics: [OperatorTotalMetric] {
		guard let lifecycleMetrics = run.lifecycleMetrics else {
			return []
		}

		return [
			contextMetric(lifecycleMetrics),
			trackerMetric(lifecycleMetrics),
			protocolMetric(lifecycleMetrics),
		].compactMap { $0 }
	}

	private func contextMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		var items = [OperatorLaneReadoutItem]()
		if metrics.inputTokensCumulative > 0 {
			items.append(OperatorLaneReadoutItem(label: "input", value: formatCompactCount(metrics.inputTokensCumulative)))
		}
		if metrics.outputTokensCumulative > 0 {
			items.append(OperatorLaneReadoutItem(label: "output", value: formatCompactCount(metrics.outputTokensCumulative)))
		}

		let current = metrics.inputTokensCurrent ?? activity?.inputTokensCurrent
		let peak = metrics.inputTokensPeak ?? activity?.inputTokensMax
		if let current {
			items.append(OperatorLaneReadoutItem(label: "current window", value: formatCompactCount(current)))
		}
		if let peak, peak != current {
			items.append(OperatorLaneReadoutItem(label: "peak window", value: formatCompactCount(peak)))
		}

		guard items.isEmpty == false else {
			return nil
		}

		return OperatorTotalMetric(
			title: "Context",
			items: items
		)
	}

	private func trackerMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
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

	private func protocolMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
		guard metrics.protocolEventCount > 0 || metrics.childEventCount > 0 else {
			return nil
		}

		var items = [OperatorLaneReadoutItem]()
		if metrics.protocolEventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(metrics.protocolEventCount)))
		}
		if metrics.childEventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "child events", value: formatCompactCount(metrics.childEventCount)))
		}

		return OperatorTotalMetric(
			title: "Protocol",
			items: items
		)
	}

	private var lifecycleTableRows: [OperatorLifecycleTableRow] {
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

	private func lifecycleTableRow(
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

	private func formatLargestOutput(bytes: Int?) -> String {
		guard let bytes, bytes > 0 else {
			return "-"
		}

		return formatCompactBytes(bytes)
	}

	private func lifecycleWallSeconds(
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

	private func runtimeShareParts(
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

	private func lifecycleModelSeconds(_ buckets: [OperatorLifecycleMetricBucket]) -> Int? {
		buckets.first { bucket in
			bucket.name.caseInsensitiveCompare("Model") == .orderedSame
		}?.wallSeconds
	}

	private func lifecycleBucket(named name: String, in buckets: [OperatorLifecycleMetricBucket]) -> OperatorLifecycleMetricBucket? {
		buckets.first { bucket in
			bucket.name.caseInsensitiveCompare(name) == .orderedSame
		}
	}

	private var bucketRows: [OperatorChildAgentBucket] {
		activity?.buckets ?? []
	}

	private var totalWallSeconds: Int {
		max(
			1,
			activity?.wallSeconds(at: currentTime) ?? 0,
			bucketRows.reduce(0) { $0 + max(0, bucketWallSeconds($1)) }
		)
	}

	private func bucketReadoutItems(_ bucket: OperatorChildAgentBucket) -> [OperatorLaneReadoutItem] {
		let normalizedName = bucket.name.lowercased()
		var items = [OperatorLaneReadoutItem]()

		let wallSeconds = bucketWallSeconds(bucket)

		if normalizedName.contains("tracker"), wallSeconds > 0 {
			items.append(OperatorLaneReadoutItem(label: "wall", value: formatActivityDuration(wallSeconds) ?? "0s"))
		}
		if bucket.eventCount > 0 {
			items.append(OperatorLaneReadoutItem(label: "events", value: formatCompactCount(bucket.eventCount)))
		}
		if normalizedName.contains("protocol") {
			if bucket.inputTokens > 0 {
				items.append(OperatorLaneReadoutItem(label: "input", value: "\(formatCompactCount(bucket.inputTokens)) tok"))
			}
			if bucket.outputTokens > 0 {
				items.append(OperatorLaneReadoutItem(label: "output", value: "\(formatCompactCount(bucket.outputTokens)) tok"))
			}
		} else {
			if bucket.toolCallCount > 0 {
				items.append(OperatorLaneReadoutItem(label: "tools", value: formatCompactCount(bucket.toolCallCount)))
			}
			if bucket.outputBytes > 0 {
				items.append(OperatorLaneReadoutItem(label: "output bytes", value: formatCompactBytes(bucket.outputBytes)))
			}
			if normalizedName.contains("tracker") == false {
				if bucket.inputTokens > 0 {
					items.append(OperatorLaneReadoutItem(label: "input", value: "\(formatCompactCount(bucket.inputTokens)) tok"))
				}
				if bucket.outputTokens > 0 {
					items.append(OperatorLaneReadoutItem(label: "output", value: "\(formatCompactCount(bucket.outputTokens)) tok"))
				}
			}
		}

		return items
	}

	private func bucketPriority(_ name: String) -> Int {
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

	private func bucketWallSeconds(_ bucket: OperatorChildAgentBucket) -> Int {
		activity?.wallSeconds(for: bucket, at: currentTime) ?? bucket.wallSeconds
	}
}

struct OperatorModelProgressReadout {
	let title: String
	let percent: Int
	let elapsed: String
	let total: String
	let barShare: CGFloat
}

fileprivate func operatorModelProgressReadout(
	for run: OperatorRunStatus,
	currentTime: Date
) -> OperatorModelProgressReadout? {
	if let lifecycleMetrics = run.lifecycleMetrics,
		let modelSeconds = operatorLifecycleModelSeconds(lifecycleMetrics.buckets)
	{
		let totalSeconds = max(
			1,
			lifecycleMetrics.wallSeconds,
			lifecycleMetrics.buckets.reduce(0) { $0 + max(0, $1.wallSeconds) },
			modelSeconds
		)
		let share = CGFloat(modelSeconds) / CGFloat(totalSeconds)

		return OperatorModelProgressReadout(
			title: "Inference",
			percent: Int((Double(modelSeconds) / Double(totalSeconds) * 100).rounded()),
			elapsed: formatActivityDuration(modelSeconds) ?? "0s",
			total: formatActivityDuration(totalSeconds) ?? "0s",
			barShare: min(1, max(0.02, share))
		)
	}

	guard let activity = run.childAgentActivity,
		let modelBucket = activity.buckets.first(where: { bucket in
			bucket.name.caseInsensitiveCompare("Model") == .orderedSame
		})
	else {
		return nil
	}

	let totalSeconds = max(
		1,
		activity.wallSeconds(at: currentTime),
		activity.buckets.reduce(0) { total, bucket in
			total + max(0, activity.wallSeconds(for: bucket, at: currentTime))
		}
	)
	let modelSeconds = activity.wallSeconds(for: modelBucket, at: currentTime)
	guard modelSeconds > 0 else {
		return nil
	}
	let share = CGFloat(modelSeconds) / CGFloat(totalSeconds)

	return OperatorModelProgressReadout(
		title: "Inference",
		percent: Int((Double(modelSeconds) / Double(totalSeconds) * 100).rounded()),
		elapsed: formatActivityDuration(modelSeconds) ?? "0s",
		total: formatActivityDuration(totalSeconds) ?? "0s",
		barShare: min(1, max(0.02, share))
	)
}

fileprivate func operatorLifecycleModelSeconds(
	_ buckets: [OperatorLifecycleMetricBucket]
) -> Int? {
	buckets.first { bucket in
		bucket.name.caseInsensitiveCompare("Model") == .orderedSame
	}?.wallSeconds
}
