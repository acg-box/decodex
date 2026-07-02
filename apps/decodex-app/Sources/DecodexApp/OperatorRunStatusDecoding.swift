import Foundation

extension OperatorRunStatus {
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
}
