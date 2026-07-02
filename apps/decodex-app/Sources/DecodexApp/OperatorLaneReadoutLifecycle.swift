import Foundation

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
