import Foundation

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
