import Foundation

extension OperatorRunStatus {
	var compactTitle: String {
		if let issueIdentifier = operatorTrimmed(issueIdentifier), issueIdentifier.isEmpty == false {
			return issueIdentifier
		}
		if let title = operatorTrimmed(title), title.isEmpty == false {
			return title
		}

		return "Run"
	}

	var compactDetail: String {
		if let currentDetail = operatorTrimmed(childAgentActivity?.currentDetail), currentDetail.isEmpty == false {
			return currentDetail
		}
		if let currentBucket = operatorTrimmed(childAgentActivity?.currentBucket), currentBucket.isEmpty == false {
			return operatorRawDisplayToken(currentBucket)
		}
		if let waitReason = operatorTrimmed(waitReason), waitReason.isEmpty == false {
			return operatorRawDisplayToken(waitReason)
		}
		if let operation = operatorTrimmed(currentOperation), operation.isEmpty == false, operation != "idle" {
			return operatorRawDisplayToken(operation)
		}
		if let runPhase = operatorTrimmed(runPhase ?? phase), runPhase.isEmpty == false {
			return operatorRawDisplayToken(runPhase)
		}
		if let threadStatus = operatorTrimmed(threadStatus), threadStatus.isEmpty == false {
			return operatorRawDisplayToken(threadStatus)
		}
		if let status = operatorTrimmed(status), status.isEmpty == false {
			return operatorRawDisplayToken(status)
		}

		return "Active"
	}

	func compactActivitySummary(at now: Date) -> String? {
		guard let activity = childAgentActivity else {
			return nil
		}

		var parts = [String]()
		if let modelBucket = activity.buckets.first(where: { $0.name.caseInsensitiveCompare("Model") == .orderedSame }) {
			let modelSeconds = activity.wallSeconds(for: modelBucket, at: now)
			if modelSeconds > 0, let formatted = formatOperatorActivityDuration(modelSeconds) {
				parts.append("Model \(formatted)")
			}
		} else if activity.wallSeconds(at: now) > 0,
			let formatted = formatOperatorActivityDuration(activity.wallSeconds(at: now))
		{
			parts.append("Activity \(formatted)")
		}

		if activity.inputTokensCumulative > 0 || activity.outputTokensCumulative > 0 {
			parts.append(
				"in \(formatOperatorCompactCount(activity.inputTokensCumulative)) / out \(formatOperatorCompactCount(activity.outputTokensCumulative))"
			)
		}
		if activity.toolCallCount > 0 {
			parts.append("\(formatOperatorCompactCount(activity.toolCallCount)) tools")
		}
		if let largestOutput = activity.largestToolOutputBytes, largestOutput > 0 {
			parts.append("\(formatOperatorCompactBytes(largestOutput)) output")
		}

		return parts.isEmpty ? nil : parts.joined(separator: " · ")
	}
}
