import Foundation

private let operatorCurrentLaneIdleTimeoutSeconds = 300

struct OperatorRunStatus: Decodable, Identifiable, Sendable {
	let projectID: String?
	let projectDisplayName: String?
	let runID: String
	let issueID: String?
	let issueIdentifier: String?
	let title: String?
	let status: String?
	let attemptStatus: String?
	let attemptNumber: Int?
	let phase: String?
	let runPhase: String?
	let waitReason: String?
	let currentOperation: String?
	let activeGoalPhase: String?
	let publicProgressPhase: String?
	let threadStatus: String?
	let threadActiveFlags: [String]
	let idleForSeconds: Int?
	let protocolIdleForSeconds: Int?
	let updatedAt: String?
	let lastProgressAt: String?
	let nextRetryAt: String?
	let lastEventType: String?
	let eventCount: Int?
	let executionLiveness: String?
	let hasFreshExecutionSnapshot: Bool?
	let countsAsRunningSnapshot: Bool?
	let needsAttentionSnapshot: Bool?
	let processAlive: Bool?
	let processLivenessReason: String?
	let runLease: Bool?
	let branchName: String?
	let worktreePath: String?
	let suspectedStall: Bool
	let childAgentActivity: OperatorChildAgentActivity?
	let lifecycleMetrics: OperatorLifecycleMetrics?
	let continuationRecovery: OperatorContinuationRecoveryStatus?
	let account: OperatorRunAccountSummary?
	let accounts: [OperatorRunAccountSummary]

	var id: String {
		runID
	}

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

	enum CodingKeys: String, CodingKey {
		case projectID = "project_id"
		case projectDisplayName = "project_display_name"
		case runID = "run_id"
		case issueID = "issue_id"
		case issueIdentifier = "issue_identifier"
		case title
		case status
		case attemptStatus = "attempt_status"
		case attemptNumber = "attempt_number"
		case phase
		case runPhase = "run_phase"
		case waitReason = "wait_reason"
		case currentOperation = "current_operation"
		case activeGoalPhase = "active_goal_phase"
		case publicProgressPhase = "public_progress_phase"
		case threadStatus = "thread_status"
		case threadActiveFlags = "thread_active_flags"
		case idleForSeconds = "idle_for_seconds"
		case protocolIdleForSeconds = "protocol_idle_for_seconds"
		case updatedAt = "updated_at"
		case lastProgressAt = "last_progress_at"
		case nextRetryAt = "next_retry_at"
		case lastEventType = "last_event_type"
		case eventCount = "event_count"
		case executionLiveness = "execution_liveness"
		case hasFreshExecutionSnapshot = "has_fresh_execution"
		case countsAsRunningSnapshot = "counts_as_running"
		case needsAttentionSnapshot = "needs_attention"
		case processAlive = "process_alive"
		case processLivenessReason = "process_liveness_reason"
		case runLease = "run_lease"
		case branchName = "branch_name"
		case worktreePath = "worktree_path"
		case suspectedStall = "suspected_stall"
		case childAgentActivity = "child_agent_activity"
		case lifecycleMetrics = "lifecycle_metrics"
		case continuationRecovery = "continuation_recovery"
		case account
		case accounts
		case codexAccount = "codex_account"
		case codexAccounts = "codex_accounts"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		projectID = try container.decodeIfPresent(String.self, forKey: .projectID)
		projectDisplayName = try container.decodeIfPresent(String.self, forKey: .projectDisplayName)
		runID = try container.decodeIfPresent(String.self, forKey: .runID) ?? UUID().uuidString
		issueID = try container.decodeIfPresent(String.self, forKey: .issueID)
		issueIdentifier = try container.decodeIfPresent(String.self, forKey: .issueIdentifier)
		title = try container.decodeIfPresent(String.self, forKey: .title)
		status = try container.decodeIfPresent(String.self, forKey: .status)
		attemptStatus = try container.decodeIfPresent(String.self, forKey: .attemptStatus)
		attemptNumber = try container.decodeIfPresent(Int.self, forKey: .attemptNumber)
		phase = try container.decodeIfPresent(String.self, forKey: .phase)
		runPhase = try container.decodeIfPresent(String.self, forKey: .runPhase)
		waitReason = try container.decodeIfPresent(String.self, forKey: .waitReason)
		currentOperation = try container.decodeIfPresent(String.self, forKey: .currentOperation)
		activeGoalPhase = try container.decodeIfPresent(String.self, forKey: .activeGoalPhase)
		publicProgressPhase = try container.decodeIfPresent(String.self, forKey: .publicProgressPhase)
		threadStatus = try container.decodeIfPresent(String.self, forKey: .threadStatus)
		threadActiveFlags = try container.decodeIfPresent([String].self, forKey: .threadActiveFlags) ?? []
		idleForSeconds = try container.decodeIfPresent(Int.self, forKey: .idleForSeconds)
		protocolIdleForSeconds = try container.decodeIfPresent(Int.self, forKey: .protocolIdleForSeconds)
		updatedAt = try container.decodeIfPresent(String.self, forKey: .updatedAt)
		lastProgressAt = try container.decodeIfPresent(String.self, forKey: .lastProgressAt)
		nextRetryAt = try container.decodeIfPresent(String.self, forKey: .nextRetryAt)
		lastEventType = try container.decodeIfPresent(String.self, forKey: .lastEventType)
		eventCount = try container.decodeIfPresent(Int.self, forKey: .eventCount)
		executionLiveness = try container.decodeIfPresent(String.self, forKey: .executionLiveness)
		hasFreshExecutionSnapshot = try container.decodeIfPresent(Bool.self, forKey: .hasFreshExecutionSnapshot)
		countsAsRunningSnapshot = try container.decodeIfPresent(Bool.self, forKey: .countsAsRunningSnapshot)
		needsAttentionSnapshot = try container.decodeIfPresent(Bool.self, forKey: .needsAttentionSnapshot)
		processAlive = try container.decodeIfPresent(Bool.self, forKey: .processAlive)
		processLivenessReason = try container.decodeIfPresent(String.self, forKey: .processLivenessReason)
		runLease = try container.decodeIfPresent(Bool.self, forKey: .runLease)
		branchName = try container.decodeIfPresent(String.self, forKey: .branchName)
		worktreePath = try container.decodeIfPresent(String.self, forKey: .worktreePath)
		suspectedStall = try container.decodeIfPresent(Bool.self, forKey: .suspectedStall) ?? false
		childAgentActivity = try container.decodeIfPresent(
			OperatorChildAgentActivity.self,
			forKey: .childAgentActivity
		)
		lifecycleMetrics = try container.decodeIfPresent(OperatorLifecycleMetrics.self, forKey: .lifecycleMetrics)
		continuationRecovery = try container.decodeIfPresent(
			OperatorContinuationRecoveryStatus.self,
			forKey: .continuationRecovery
		)
		account = try container.decodeIfPresent(OperatorRunAccountSummary.self, forKey: .account)
			?? container.decodeIfPresent(OperatorRunAccountSummary.self, forKey: .codexAccount)
		accounts = try container.decodeIfPresent([OperatorRunAccountSummary].self, forKey: .accounts)
			?? container.decodeIfPresent([OperatorRunAccountSummary].self, forKey: .codexAccounts)
			?? []
	}

	private init(
		projectID: String?,
		projectDisplayName: String?,
		runID: String,
		issueID: String?,
		issueIdentifier: String?,
		title: String?,
		status: String?,
		attemptStatus: String?,
		attemptNumber: Int?,
		phase: String?,
		runPhase: String?,
		waitReason: String?,
		currentOperation: String?,
		activeGoalPhase: String?,
		publicProgressPhase: String?,
		threadStatus: String?,
		threadActiveFlags: [String],
		idleForSeconds: Int?,
		protocolIdleForSeconds: Int?,
		updatedAt: String?,
		lastProgressAt: String?,
		nextRetryAt: String?,
		lastEventType: String?,
		eventCount: Int?,
		executionLiveness: String?,
		hasFreshExecutionSnapshot: Bool?,
		countsAsRunningSnapshot: Bool?,
		needsAttentionSnapshot: Bool?,
		processAlive: Bool?,
		processLivenessReason: String?,
		runLease: Bool?,
		branchName: String?,
		worktreePath: String?,
		suspectedStall: Bool,
		childAgentActivity: OperatorChildAgentActivity?,
		lifecycleMetrics: OperatorLifecycleMetrics?,
		continuationRecovery: OperatorContinuationRecoveryStatus?,
		account: OperatorRunAccountSummary?,
		accounts: [OperatorRunAccountSummary]
	) {
		self.projectID = projectID
		self.projectDisplayName = projectDisplayName
		self.runID = runID
		self.issueID = issueID
		self.issueIdentifier = issueIdentifier
		self.title = title
		self.status = status
		self.attemptStatus = attemptStatus
		self.attemptNumber = attemptNumber
		self.phase = phase
		self.runPhase = runPhase
		self.waitReason = waitReason
		self.currentOperation = currentOperation
		self.activeGoalPhase = activeGoalPhase
		self.publicProgressPhase = publicProgressPhase
		self.threadStatus = threadStatus
		self.threadActiveFlags = threadActiveFlags
		self.idleForSeconds = idleForSeconds
		self.protocolIdleForSeconds = protocolIdleForSeconds
		self.updatedAt = updatedAt
		self.lastProgressAt = lastProgressAt
		self.nextRetryAt = nextRetryAt
		self.lastEventType = lastEventType
		self.eventCount = eventCount
		self.executionLiveness = executionLiveness
		self.hasFreshExecutionSnapshot = hasFreshExecutionSnapshot
		self.countsAsRunningSnapshot = countsAsRunningSnapshot
		self.needsAttentionSnapshot = needsAttentionSnapshot
		self.processAlive = processAlive
		self.processLivenessReason = processLivenessReason
		self.runLease = runLease
		self.branchName = branchName
		self.worktreePath = worktreePath
		self.suspectedStall = suspectedStall
		self.childAgentActivity = childAgentActivity
		self.lifecycleMetrics = lifecycleMetrics
		self.continuationRecovery = continuationRecovery
		self.account = account
		self.accounts = accounts
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
