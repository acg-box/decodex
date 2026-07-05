import Foundation

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

	private func detailBucketIsVisible(_ bucket: OperatorChildAgentBucket) -> Bool {
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

	fileprivate func formatLargestOutput(bytes: Int?) -> String {
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

	fileprivate func lifecycleBucket(
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

	private func contextMetric(_ metrics: OperatorLifecycleMetrics) -> OperatorTotalMetric? {
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
