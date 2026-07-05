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

func rawPanelToken(_ value: String) -> String {
	value.trimmingCharacters(in: .whitespacesAndNewlines)
}
