import Foundation
import SwiftUI

struct OperatorModelProgressReadout {
	let title: String
	let percent: Int
	let elapsed: String
	let total: String
	let barShare: CGFloat
}

func operatorModelProgressReadout(
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

private func operatorLifecycleModelSeconds(
	_ buckets: [OperatorLifecycleMetricBucket]
) -> Int? {
	buckets.first { bucket in
		bucket.name.caseInsensitiveCompare("Model") == .orderedSame
	}?.wallSeconds
}
