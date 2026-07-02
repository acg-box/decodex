import Foundation

private let operatorCurrentLaneIdleTimeoutSeconds = 300

extension OperatorRunStatus {
	var hasAttentionTone: Bool {
		if let needsAttentionSnapshot {
			return needsAttentionSnapshot
		}

		return suspectedStall
			|| attemptStatus == "waiting_for_review"
			|| status == "manual_attention"
			|| status == "blocked"
			|| stoppedProcessNeedsAttention
	}

	var isWaiting: Bool {
		waitReason != nil
			|| attemptStatus?.contains("waiting") == true
			|| phase?.contains("waiting") == true
	}

	var inactiveDurationSeconds: Int? {
		let candidates = [
			idleForSeconds,
			protocolIdleForSeconds,
			childAgentActivity?.currentElapsedSeconds,
		].compactMap { $0 }

		return candidates.max()
	}

	func isAssigned(to account: CodexAccount) -> Bool {
		self.account?.matches(account) == true
			|| accounts.contains { $0.isSelected && $0.matches(account) }
	}

	var countsAsRunning: Bool {
		if let countsAsRunningSnapshot {
			return countsAsRunningSnapshot
		}

		return hasRunningStatus
			&& phase == "executing"
			&& (processAlive != false || hasFreshExecution)
			&& hasAttentionTone == false
			&& hasStaleExecutionWithoutKnownProcess == false
	}

	var hasFreshExecution: Bool {
		if let hasFreshExecutionSnapshot {
			return hasFreshExecutionSnapshot
		}

		return hasRunningStatus
			&& (processAlive == true || hasRecentAppServerExecution)
	}

	private var hasRecentAppServerExecution: Bool {
		threadStatus == "active"
			|| threadActiveFlags.isEmpty == false
			|| ["thread_active", "protocol_observed"].contains(executionLiveness ?? "")
			|| protocolIdleForSeconds.isSomeAndLessThan(operatorCurrentLaneIdleTimeoutSeconds)
	}

	private var hasRunningStatus: Bool {
		guard let status else {
			return false
		}

		return ["starting", "running"].contains(status)
	}

	private var stoppedProcessNeedsAttention: Bool {
		processAlive == false
			&& hasRunningStatus
			&& waitReason == nil
			&& hasFreshExecution == false
	}

	private var hasStaleExecutionWithoutKnownProcess: Bool {
		hasRunningStatus
			&& phase == "executing"
			&& waitReason == nil
			&& processAlive != true
			&& [idleForSeconds, protocolIdleForSeconds].contains { idleForSeconds in
				guard let idleForSeconds else {
					return false
				}

				return idleForSeconds >= 300
			}
	}
}

private extension Optional where Wrapped == Int {
	func isSomeAndLessThan(_ threshold: Int) -> Bool {
		guard let value = self else {
			return false
		}

		return value < threshold
	}
}
