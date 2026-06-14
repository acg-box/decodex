import Foundation

struct OperatorSnapshotResponse: Decodable, Sendable {
	let warnings: [String]
	let projects: [OperatorProjectStatus]
	let activeRuns: [OperatorRunStatus]
	let queuedCandidates: [OperatorQueuedIssueStatus]
	let postReviewLanes: [OperatorPostReviewLaneStatus]

	var activeRunCount: Int {
		max(
			activeRuns.count,
			projects.reduce(0) { $0 + $1.activeRunCount }
		)
	}

	var runningLaneCount: Int {
		max(
			activeRuns.filter(\.countsAsRunning).count,
			projects.reduce(0) { $0 + $1.runningLaneCount }
		)
	}

	var queuedCount: Int {
		max(
			queuedCandidates.filter { $0.isClosed == false }.count,
			projects.reduce(0) { $0 + $1.queuedCandidateCount }
		)
	}

	var reviewCount: Int {
		max(
			postReviewLanes.count,
			projects.reduce(0) { $0 + $1.postReviewLaneCount }
		)
	}

	var landingCount: Int {
		postReviewLanes.filter { $0.isReadyToLand }.count
	}

	var waitingCount: Int {
		projects.reduce(0) { $0 + $1.waitingLaneCount }
	}

	var attentionCount: Int {
		projects.reduce(0) { $0 + $1.attentionCount }
	}

	var cleanupCount: Int {
		projects.reduce(0) { $0 + $1.cleanupBlockedCount + $1.cleanupPendingCount }
	}

	var hasVisibleSignal: Bool {
		activeRunCount > 0
			|| queuedCount > 0
			|| waitingCount > 0
			|| attentionCount > 0
			|| cleanupCount > 0
			|| warnings.isEmpty == false
	}

	var shouldDisplayInPanel: Bool {
		hasVisibleSignal && (activeRunCount > 0 || isDevSnapshot == false)
	}

	var warningSummary: String? {
		let labels = warnings
			.filter { $0 != "automation_disabled" }
			.map(rawDisplayToken)
			.filter { $0.isEmpty == false }
		guard let first = labels.first else {
			return nil
		}
		if labels.count == 1 {
			return first
		}

		return "\(first) +\(labels.count - 1)"
	}

	func activeRuns(for account: CodexAccount) -> [OperatorRunStatus] {
		activeRuns.filter { $0.isAssigned(to: account) }
	}

	func runningCount(for account: CodexAccount) -> Int {
		activeRuns(for: account).filter(\.countsAsRunning).count
	}

	func mergingRunActivity(
		_ activityRuns: [OperatorRunStatus],
		activeRunsComplete: Bool = true
	) -> OperatorSnapshotResponse {
		let activityRunsByID = activityRuns.reduce(into: [String: OperatorRunStatus]()) { runsByID, run in
			runsByID[run.runID] = run
		}
		let snapshotRunsByID = activeRuns.reduce(into: [String: OperatorRunStatus]()) { runsByID, run in
			runsByID[run.runID] = run
		}
		let mergedSnapshotRuns = activeRuns.compactMap { snapshotRun -> OperatorRunStatus? in
			if let activityRun = activityRunsByID[snapshotRun.runID] {
				return snapshotRun.mergingActivity(activityRun)
			}
			if activeRunsComplete {
				return nil
			}

			return snapshotRun.shouldRetainDuringPartialRunActivity ? snapshotRun : nil
		}
		let newActivityRuns = activityRuns.filter { activityRun in
			snapshotRunsByID[activityRun.runID] == nil
		}
		let mergedRuns = mergedSnapshotRuns + newActivityRuns
		let activeCountsByProject = Dictionary(grouping: mergedRuns.compactMap(\.projectID)) { $0 }
			.mapValues(\.count)
		let runningCountsByProject = Dictionary(
			grouping: mergedRuns.filter(\.countsAsRunning).compactMap(\.projectID)
		) { $0 }
			.mapValues(\.count)
		let mergedProjects = projects.map { project in
			guard let projectID = project.projectID else {
				return project
			}

			return project.withRunCounts(
				active: activeCountsByProject[projectID] ?? 0,
				running: runningCountsByProject[projectID] ?? 0
			)
		}

		return OperatorSnapshotResponse(
			warnings: warnings,
			projects: mergedProjects,
			activeRuns: mergedRuns,
			queuedCandidates: queuedCandidates,
			postReviewLanes: postReviewLanes
		)
	}

	static func activeRunsOnly(_ activeRuns: [OperatorRunStatus]) -> OperatorSnapshotResponse {
		OperatorSnapshotResponse(
			warnings: [],
			projects: [],
			activeRuns: activeRuns,
			queuedCandidates: [],
			postReviewLanes: []
		)
	}

	private init(
		warnings: [String],
		projects: [OperatorProjectStatus],
		activeRuns: [OperatorRunStatus],
		queuedCandidates: [OperatorQueuedIssueStatus],
		postReviewLanes: [OperatorPostReviewLaneStatus]
	) {
		self.warnings = warnings
		self.projects = projects
		self.activeRuns = activeRuns
		self.queuedCandidates = queuedCandidates
		self.postReviewLanes = postReviewLanes
	}

	private var isDevSnapshot: Bool {
		warnings.contains("automation_disabled")
			&& projects.allSatisfy { $0.connectorState == "api_only" || $0.connectorState == "dev" }
	}

	enum CodingKeys: String, CodingKey {
		case warnings
		case projects
		case activeRuns = "active_runs"
		case queuedCandidates = "queued_candidates"
		case postReviewLanes = "post_review_lanes"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		warnings = try container.decodeIfPresent([String].self, forKey: .warnings) ?? []
		projects = try container.decodeIfPresent([OperatorProjectStatus].self, forKey: .projects) ?? []
		activeRuns = try container.decodeIfPresent([OperatorRunStatus].self, forKey: .activeRuns) ?? []
		queuedCandidates = try container.decodeIfPresent(
			[OperatorQueuedIssueStatus].self,
			forKey: .queuedCandidates
		) ?? []
		postReviewLanes = try container.decodeIfPresent(
			[OperatorPostReviewLaneStatus].self,
			forKey: .postReviewLanes
		) ?? []
	}
}

struct OperatorProjectStatus: Decodable, Sendable {
	let projectID: String?
	let enabled: Bool
	let connectorState: String?
	let warningCount: Int
	let activeRunCount: Int
	let runningLaneCount: Int
	let queuedCandidateCount: Int
	let postReviewLaneCount: Int
	let waitingLaneCount: Int
	let attentionCount: Int
	let cleanupBlockedCount: Int
	let cleanupPendingCount: Int

	func withRunCounts(active: Int, running: Int) -> OperatorProjectStatus {
		OperatorProjectStatus(
			projectID: projectID,
			enabled: enabled,
			connectorState: connectorState,
			warningCount: warningCount,
			activeRunCount: active,
			runningLaneCount: running,
			queuedCandidateCount: queuedCandidateCount,
			postReviewLaneCount: postReviewLaneCount,
			waitingLaneCount: waitingLaneCount,
			attentionCount: attentionCount,
			cleanupBlockedCount: cleanupBlockedCount,
			cleanupPendingCount: cleanupPendingCount
		)
	}

	enum CodingKeys: String, CodingKey {
		case projectID = "project_id"
		case enabled
		case connectorState = "connector_state"
		case warningCount = "warning_count"
		case activeRunCount = "active_run_count"
		case runningLaneCount = "running_lane_count"
		case queuedCandidateCount = "queued_candidate_count"
		case postReviewLaneCount = "post_review_lane_count"
		case waitingLaneCount = "waiting_lane_count"
		case attentionCount = "attention_count"
		case cleanupBlockedCount = "cleanup_blocked_count"
		case cleanupPendingCount = "cleanup_pending_count"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		projectID = try container.decodeIfPresent(String.self, forKey: .projectID)
		enabled = try container.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
		connectorState = try container.decodeIfPresent(String.self, forKey: .connectorState)
		warningCount = try container.decodeIfPresent(Int.self, forKey: .warningCount) ?? 0
		activeRunCount = try container.decodeIfPresent(Int.self, forKey: .activeRunCount) ?? 0
		runningLaneCount =
			try container.decodeIfPresent(Int.self, forKey: .runningLaneCount) ?? activeRunCount
		queuedCandidateCount = try container.decodeIfPresent(Int.self, forKey: .queuedCandidateCount) ?? 0
		postReviewLaneCount = try container.decodeIfPresent(Int.self, forKey: .postReviewLaneCount) ?? 0
		waitingLaneCount = try container.decodeIfPresent(Int.self, forKey: .waitingLaneCount) ?? 0
		attentionCount = try container.decodeIfPresent(Int.self, forKey: .attentionCount) ?? 0
		cleanupBlockedCount = try container.decodeIfPresent(Int.self, forKey: .cleanupBlockedCount) ?? 0
		cleanupPendingCount = try container.decodeIfPresent(Int.self, forKey: .cleanupPendingCount) ?? 0
	}

	private init(
		projectID: String?,
		enabled: Bool,
		connectorState: String?,
		warningCount: Int,
		activeRunCount: Int,
		runningLaneCount: Int,
		queuedCandidateCount: Int,
		postReviewLaneCount: Int,
		waitingLaneCount: Int,
		attentionCount: Int,
		cleanupBlockedCount: Int,
		cleanupPendingCount: Int
	) {
		self.projectID = projectID
		self.enabled = enabled
		self.connectorState = connectorState
		self.warningCount = warningCount
		self.activeRunCount = activeRunCount
		self.runningLaneCount = runningLaneCount
		self.queuedCandidateCount = queuedCandidateCount
		self.postReviewLaneCount = postReviewLaneCount
		self.waitingLaneCount = waitingLaneCount
		self.attentionCount = attentionCount
		self.cleanupBlockedCount = cleanupBlockedCount
		self.cleanupPendingCount = cleanupPendingCount
	}
}

struct OperatorQueuedIssueStatus: Decodable, Sendable {
	let classification: String?

	var isClosed: Bool {
		classification == "closed"
	}

	enum CodingKeys: String, CodingKey {
		case classification
	}
}

struct OperatorPostReviewLaneStatus: Decodable, Sendable {
	let classification: String?

	var isReadyToLand: Bool {
		classification == "ready_to_land"
	}

	enum CodingKeys: String, CodingKey {
		case classification
	}
}

struct OperatorLifecycleMetricBucket: Decodable, Sendable {
	let name: String
	let wallSeconds: Int
	let eventCount: Int
	let toolCallCount: Int
	let inputTokens: Int
	let outputTokens: Int
	let outputBytes: Int

	enum CodingKeys: String, CodingKey {
		case name
		case wallSeconds = "wall_seconds"
		case eventCount = "event_count"
		case toolCallCount = "tool_call_count"
		case inputTokens = "input_tokens"
		case outputTokens = "output_tokens"
		case outputBytes = "output_bytes"
	}
}

struct OperatorLifecycleMetricPhase: Decodable, Sendable {
	let phase: String?
	let label: String?
	let attemptCount: Int
	let runCount: Int
	let capturedAttemptCount: Int
	let missingAttemptCount: Int
	let protocolEventCount: Int
	let childEventCount: Int
	let wallSeconds: Int
	let toolCallCount: Int
	let inputTokensCurrent: Int?
	let inputTokensPeak: Int?
	let inputTokensCumulative: Int
	let outputTokensCumulative: Int
	let largestToolOutputBytes: Int?
	let largestToolOutputTool: String?
	let buckets: [OperatorLifecycleMetricBucket]

	enum CodingKeys: String, CodingKey {
		case phase
		case label
		case attemptCount = "attempt_count"
		case runCount = "run_count"
		case capturedAttemptCount = "captured_attempt_count"
		case missingAttemptCount = "missing_attempt_count"
		case protocolEventCount = "protocol_event_count"
		case childEventCount = "child_event_count"
		case wallSeconds = "wall_seconds"
		case toolCallCount = "tool_call_count"
		case inputTokensCurrent = "input_tokens_current"
		case inputTokensPeak = "input_tokens_peak"
		case inputTokensCumulative = "input_tokens_cumulative"
		case outputTokensCumulative = "output_tokens_cumulative"
		case largestToolOutputBytes = "largest_tool_output_bytes"
		case largestToolOutputTool = "largest_tool_output_tool"
		case buckets
	}
}

struct OperatorLifecycleMetrics: Decodable, Sendable {
	let attemptCount: Int
	let runCount: Int
	let capturedAttemptCount: Int
	let missingAttemptCount: Int
	let protocolEventCount: Int
	let childEventCount: Int
	let wallSeconds: Int
	let toolCallCount: Int
	let inputTokensCurrent: Int?
	let inputTokensPeak: Int?
	let inputTokensCumulative: Int
	let outputTokensCumulative: Int
	let largestToolOutputBytes: Int?
	let largestToolOutputTool: String?
	let buckets: [OperatorLifecycleMetricBucket]
	let phases: [OperatorLifecycleMetricPhase]

	enum CodingKeys: String, CodingKey {
		case attemptCount = "attempt_count"
		case runCount = "run_count"
		case capturedAttemptCount = "captured_attempt_count"
		case missingAttemptCount = "missing_attempt_count"
		case protocolEventCount = "protocol_event_count"
		case childEventCount = "child_event_count"
		case wallSeconds = "wall_seconds"
		case toolCallCount = "tool_call_count"
		case inputTokensCurrent = "input_tokens_current"
		case inputTokensPeak = "input_tokens_peak"
		case inputTokensCumulative = "input_tokens_cumulative"
		case outputTokensCumulative = "output_tokens_cumulative"
		case largestToolOutputBytes = "largest_tool_output_bytes"
		case largestToolOutputTool = "largest_tool_output_tool"
		case buckets
		case phases
	}
}

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
	let waitReason: String?
	let currentOperation: String?
	let threadStatus: String?
	let idleForSeconds: Int?
	let protocolIdleForSeconds: Int?
	let updatedAt: String?
	let lastProgressAt: String?
	let nextRetryAt: String?
	let lastEventType: String?
	let eventCount: Int?
	let processAlive: Bool?
	let activeLease: Bool?
	let branchName: String?
	let worktreePath: String?
	let suspectedStall: Bool
	let childAgentActivity: OperatorChildAgentActivity?
	let lifecycleMetrics: OperatorLifecycleMetrics?
	let account: OperatorRunAccountSummary?
	let accounts: [OperatorRunAccountSummary]

	var id: String {
		runID
	}

	var compactTitle: String {
		if let issueIdentifier = trimmed(issueIdentifier), issueIdentifier.isEmpty == false {
			return issueIdentifier
		}
		if let title = trimmed(title), title.isEmpty == false {
			return title
		}

		return "Run"
	}

	var compactDetail: String {
		if let currentDetail = trimmed(childAgentActivity?.currentDetail), currentDetail.isEmpty == false {
			return currentDetail
		}
		if let currentBucket = trimmed(childAgentActivity?.currentBucket), currentBucket.isEmpty == false {
			return rawDisplayToken(currentBucket)
		}
		if let waitReason = trimmed(waitReason), waitReason.isEmpty == false {
			return rawDisplayToken(waitReason)
		}
		if let operation = trimmed(currentOperation), operation.isEmpty == false, operation != "idle" {
			return rawDisplayToken(operation)
		}
		if let phase = trimmed(phase), phase.isEmpty == false {
			return rawDisplayToken(phase)
		}
		if let threadStatus = trimmed(threadStatus), threadStatus.isEmpty == false {
			return rawDisplayToken(threadStatus)
		}
		if let status = trimmed(status), status.isEmpty == false {
			return rawDisplayToken(status)
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
		suspectedStall
			|| attemptStatus == "waiting_for_review"
			|| status == "manual_attention"
			|| status == "blocked"
			|| processAlive == false
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

	var shouldRetainDuringPartialRunActivity: Bool {
		activeLease == true
			|| processAlive == true
			|| status == "running"
			|| phase == "executing"
	}

	var countsAsRunning: Bool {
		hasRunningStatus
			&& phase == "executing"
			&& processAlive != false
			&& hasAttentionTone == false
			&& hasStaleExecutionWithoutKnownProcess == false
	}

	private var hasRunningStatus: Bool {
		guard let status else {
			return false
		}

		return ["starting", "running"].contains(status)
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

	func mergingActivity(_ activity: OperatorRunStatus) -> OperatorRunStatus {
		OperatorRunStatus(
			projectID: activity.projectID ?? projectID,
			projectDisplayName: activity.projectDisplayName ?? projectDisplayName,
			runID: activity.runID,
			issueID: activity.issueID ?? issueID,
			issueIdentifier: activity.issueIdentifier ?? issueIdentifier,
			title: mergedTitle(from: activity),
			status: activity.status ?? status,
			attemptStatus: activity.attemptStatus ?? attemptStatus,
			attemptNumber: activity.attemptNumber ?? attemptNumber,
			phase: activity.phase ?? phase,
			waitReason: activity.waitReason ?? waitReason,
			currentOperation: activity.currentOperation ?? currentOperation,
			threadStatus: activity.threadStatus ?? threadStatus,
			idleForSeconds: activity.idleForSeconds ?? idleForSeconds,
			protocolIdleForSeconds: activity.protocolIdleForSeconds ?? protocolIdleForSeconds,
			updatedAt: activity.updatedAt ?? updatedAt,
			lastProgressAt: activity.lastProgressAt ?? lastProgressAt,
			nextRetryAt: activity.nextRetryAt ?? nextRetryAt,
			lastEventType: activity.lastEventType ?? lastEventType,
			eventCount: activity.eventCount ?? eventCount,
			processAlive: activity.processAlive ?? processAlive,
			activeLease: activity.activeLease ?? activeLease,
			branchName: activity.branchName ?? branchName,
			worktreePath: activity.worktreePath ?? worktreePath,
			suspectedStall: activity.suspectedStall || suspectedStall,
			childAgentActivity: activity.childAgentActivity ?? childAgentActivity,
			lifecycleMetrics: activity.lifecycleMetrics ?? lifecycleMetrics,
			account: activity.account ?? account,
			accounts: activity.accounts.isEmpty ? accounts : activity.accounts
		)
	}

	private func mergedTitle(from activity: OperatorRunStatus) -> String? {
		if let title = activity.title, title.isEmpty == false, activity.titleIsOperationFallback == false {
			return title
		}

		return title ?? activity.title
	}

	private var titleIsOperationFallback: Bool {
		guard let title, title.isEmpty == false else {
			return false
		}

		return title == rawDisplayToken(currentOperation ?? phase ?? "")
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
		case waitReason = "wait_reason"
		case currentOperation = "current_operation"
		case threadStatus = "thread_status"
		case idleForSeconds = "idle_for_seconds"
		case protocolIdleForSeconds = "protocol_idle_for_seconds"
		case updatedAt = "updated_at"
		case lastProgressAt = "last_progress_at"
		case nextRetryAt = "next_retry_at"
		case lastEventType = "last_event_type"
		case eventCount = "event_count"
		case processAlive = "process_alive"
		case activeLease = "active_lease"
		case branchName = "branch_name"
		case worktreePath = "worktree_path"
		case suspectedStall = "suspected_stall"
		case childAgentActivity = "child_agent_activity"
		case lifecycleMetrics = "lifecycle_metrics"
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
		waitReason = try container.decodeIfPresent(String.self, forKey: .waitReason)
		currentOperation = try container.decodeIfPresent(String.self, forKey: .currentOperation)
		threadStatus = try container.decodeIfPresent(String.self, forKey: .threadStatus)
		idleForSeconds = try container.decodeIfPresent(Int.self, forKey: .idleForSeconds)
		protocolIdleForSeconds = try container.decodeIfPresent(Int.self, forKey: .protocolIdleForSeconds)
		updatedAt = try container.decodeIfPresent(String.self, forKey: .updatedAt)
		lastProgressAt = try container.decodeIfPresent(String.self, forKey: .lastProgressAt)
		nextRetryAt = try container.decodeIfPresent(String.self, forKey: .nextRetryAt)
		lastEventType = try container.decodeIfPresent(String.self, forKey: .lastEventType)
		eventCount = try container.decodeIfPresent(Int.self, forKey: .eventCount)
		processAlive = try container.decodeIfPresent(Bool.self, forKey: .processAlive)
		activeLease = try container.decodeIfPresent(Bool.self, forKey: .activeLease)
		branchName = try container.decodeIfPresent(String.self, forKey: .branchName)
		worktreePath = try container.decodeIfPresent(String.self, forKey: .worktreePath)
		suspectedStall = try container.decodeIfPresent(Bool.self, forKey: .suspectedStall) ?? false
		childAgentActivity = try container.decodeIfPresent(
			OperatorChildAgentActivity.self,
			forKey: .childAgentActivity
		)
		lifecycleMetrics = try container.decodeIfPresent(OperatorLifecycleMetrics.self, forKey: .lifecycleMetrics)
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
		waitReason: String?,
		currentOperation: String?,
		threadStatus: String?,
		idleForSeconds: Int?,
		protocolIdleForSeconds: Int?,
		updatedAt: String?,
		lastProgressAt: String?,
		nextRetryAt: String?,
		lastEventType: String?,
		eventCount: Int?,
		processAlive: Bool?,
		activeLease: Bool?,
		branchName: String?,
		worktreePath: String?,
		suspectedStall: Bool,
		childAgentActivity: OperatorChildAgentActivity?,
		lifecycleMetrics: OperatorLifecycleMetrics?,
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
		self.waitReason = waitReason
		self.currentOperation = currentOperation
		self.threadStatus = threadStatus
		self.idleForSeconds = idleForSeconds
		self.protocolIdleForSeconds = protocolIdleForSeconds
		self.updatedAt = updatedAt
		self.lastProgressAt = lastProgressAt
		self.nextRetryAt = nextRetryAt
		self.lastEventType = lastEventType
		self.eventCount = eventCount
		self.processAlive = processAlive
		self.activeLease = activeLease
		self.branchName = branchName
		self.worktreePath = worktreePath
		self.suspectedStall = suspectedStall
		self.childAgentActivity = childAgentActivity
		self.lifecycleMetrics = lifecycleMetrics
		self.account = account
		self.accounts = accounts
	}
}

struct OperatorDashboardSocketEvent: Decodable, Sendable {
	let type: String
	let payload: OperatorDashboardSocketPayload?
}

struct OperatorDashboardSocketPayload: Decodable, Sendable {
	let emittedAtUnixEpoch: Int64?
	let snapshotPublishedAtUnixEpoch: Int64?
	let snapshot: OperatorSnapshotResponse?
	let activeRuns: [OperatorRunStatus]?
	let activeRunsComplete: Bool?

	var emittedAt: Date? {
		date(fromUnixEpoch: emittedAtUnixEpoch)
	}

	var snapshotPublishedAt: Date? {
		date(fromUnixEpoch: snapshotPublishedAtUnixEpoch)
	}

	enum CodingKeys: String, CodingKey {
		case emittedAtUnixEpoch
		case snapshotPublishedAtUnixEpoch
		case snapshot
		case activeRuns
		case activeRunsComplete
	}
}

struct OperatorRunActivitySnapshot: Sendable {
	let activeRuns: [OperatorRunStatus]
	let activeRunsComplete: Bool
	let emittedAt: Date

	func merging(into snapshot: OperatorSnapshotResponse) -> OperatorSnapshotResponse {
		snapshot.mergingRunActivity(activeRuns, activeRunsComplete: activeRunsComplete)
	}
}

struct OperatorChildAgentActivity: Decodable, Sendable {
	let currentBucket: String?
	let currentDetail: String?
	let currentElapsedSeconds: Int?
	let currentStartedUnixEpoch: Int64?
	let eventCount: Int
	let inputTokensCumulative: Int
	let inputTokensCurrent: Int?
	let inputTokensMax: Int?
	let largestToolOutputBytes: Int?
	let largestToolOutputTool: String?
	let outputTokensCumulative: Int
	let toolCallCount: Int
	let wallSeconds: Int
	let buckets: [OperatorChildAgentBucket]

	enum CodingKeys: String, CodingKey {
		case currentBucket = "current_bucket"
		case currentDetail = "current_detail"
		case currentElapsedSeconds = "current_elapsed_seconds"
		case currentStartedUnixEpoch = "current_started_unix_epoch"
		case eventCount = "event_count"
		case inputTokensCumulative = "input_tokens_cumulative"
		case inputTokensCurrent = "input_tokens_current"
		case inputTokensMax = "input_tokens_max"
		case largestToolOutputBytes = "largest_tool_output_bytes"
		case largestToolOutputTool = "largest_tool_output_tool"
		case outputTokensCumulative = "output_tokens_cumulative"
		case toolCallCount = "tool_call_count"
		case wallSeconds = "wall_seconds"
		case buckets
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		currentBucket = try container.decodeIfPresent(String.self, forKey: .currentBucket)
		currentDetail = try container.decodeIfPresent(String.self, forKey: .currentDetail)
		currentElapsedSeconds = try container.decodeIfPresent(Int.self, forKey: .currentElapsedSeconds)
		currentStartedUnixEpoch = try container.decodeIfPresent(Int64.self, forKey: .currentStartedUnixEpoch)
		eventCount = try container.decodeIfPresent(Int.self, forKey: .eventCount) ?? 0
		inputTokensCumulative = try container.decodeIfPresent(Int.self, forKey: .inputTokensCumulative) ?? 0
		inputTokensCurrent = try container.decodeIfPresent(Int.self, forKey: .inputTokensCurrent)
		inputTokensMax = try container.decodeIfPresent(Int.self, forKey: .inputTokensMax)
		largestToolOutputBytes = try container.decodeIfPresent(Int.self, forKey: .largestToolOutputBytes)
		largestToolOutputTool = try container.decodeIfPresent(String.self, forKey: .largestToolOutputTool)
		outputTokensCumulative = try container.decodeIfPresent(Int.self, forKey: .outputTokensCumulative) ?? 0
		toolCallCount = try container.decodeIfPresent(Int.self, forKey: .toolCallCount) ?? 0
		wallSeconds = try container.decodeIfPresent(Int.self, forKey: .wallSeconds) ?? 0
		buckets = try container.decodeIfPresent([OperatorChildAgentBucket].self, forKey: .buckets) ?? []
	}

	func currentElapsedSeconds(at now: Date) -> Int? {
		var candidates = [Int]()
		if let currentElapsedSeconds {
			candidates.append(currentElapsedSeconds)
		}
		if let currentStartedUnixEpoch {
			let liveElapsed = Int(now.timeIntervalSince1970.rounded(.down)) - Int(currentStartedUnixEpoch)

			candidates.append(max(0, liveElapsed))
		}

		return candidates.max()
	}

	func wallSeconds(at now: Date) -> Int {
		wallSeconds + currentElapsedDelta(at: now)
	}

	func wallSeconds(
		for bucket: OperatorChildAgentBucket,
		at now: Date
	) -> Int {
		guard let currentBucket, bucket.name.caseInsensitiveCompare(currentBucket) == .orderedSame else {
			return bucket.wallSeconds
		}

		return bucket.wallSeconds + currentElapsedDelta(at: now)
	}

	private func currentElapsedDelta(at now: Date) -> Int {
		guard let baselineElapsed = currentElapsedSeconds, let liveElapsed = currentElapsedSeconds(at: now) else {
			return 0
		}

		return max(0, liveElapsed - baselineElapsed)
	}
}

struct OperatorChildAgentBucket: Decodable, Identifiable, Sendable {
	let name: String
	let eventCount: Int
	let inputTokens: Int
	let outputBytes: Int
	let outputTokens: Int
	let toolCallCount: Int
	let wallSeconds: Int

	var id: String {
		name
	}

	enum CodingKeys: String, CodingKey {
		case name
		case eventCount = "event_count"
		case inputTokens = "input_tokens"
		case outputBytes = "output_bytes"
		case outputTokens = "output_tokens"
		case toolCallCount = "tool_call_count"
		case wallSeconds = "wall_seconds"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		name = try container.decodeIfPresent(String.self, forKey: .name) ?? "Activity"
		eventCount = try container.decodeIfPresent(Int.self, forKey: .eventCount) ?? 0
		inputTokens = try container.decodeIfPresent(Int.self, forKey: .inputTokens) ?? 0
		outputBytes = try container.decodeIfPresent(Int.self, forKey: .outputBytes) ?? 0
		outputTokens = try container.decodeIfPresent(Int.self, forKey: .outputTokens) ?? 0
		toolCallCount = try container.decodeIfPresent(Int.self, forKey: .toolCallCount) ?? 0
		wallSeconds = try container.decodeIfPresent(Int.self, forKey: .wallSeconds) ?? 0
	}
}

struct OperatorRunAccountSummary: Decodable, Sendable {
	let accountFingerprint: String
	let email: String?
	let status: String?

	var isSelected: Bool {
		status?.caseInsensitiveCompare("selected") == .orderedSame
	}

	func matches(_ account: CodexAccount) -> Bool {
		if accountFingerprint.isEmpty == false, accountFingerprint == account.accountFingerprint {
			return true
		}
		if let email, let accountEmail = account.email {
			return email.caseInsensitiveCompare(accountEmail) == .orderedSame
		}

		return false
	}

	enum CodingKeys: String, CodingKey {
		case accountFingerprint = "account_fingerprint"
		case email
		case accountEmail = "account_email"
		case status
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		accountFingerprint = try container.decodeIfPresent(String.self, forKey: .accountFingerprint) ?? ""
		email = try container.decodeIfPresent(String.self, forKey: .email)
			?? container.decodeIfPresent(String.self, forKey: .accountEmail)
		status = try container.decodeIfPresent(String.self, forKey: .status)
	}
}

private func trimmed(_ value: String?) -> String? {
	value?.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func rawDisplayToken(_ value: String) -> String {
	value.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func formatOperatorActivityDuration(_ seconds: Int?) -> String? {
	guard let seconds else {
		return nil
	}

	let value = max(0, seconds)
	if value < 60 {
		return "\(value)s"
	}

	let hours = value / 3_600
	let minutes = (value % 3_600) / 60
	let remainderSeconds = value % 60
	if hours > 0 {
		return minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
	}
	if minutes > 0 {
		return remainderSeconds > 0 ? "\(minutes)m \(remainderSeconds)s" : "\(minutes)m"
	}

	return "\(remainderSeconds)s"
}

private func formatOperatorCompactCount(_ value: Int) -> String {
	let absoluteValue = abs(Double(value))
	let sign = value < 0 ? "-" : ""

	if absoluteValue >= 1_000_000_000 {
		return "\(sign)\(formatOperatorCompactDecimal(absoluteValue / 1_000_000_000))B"
	}
	if absoluteValue >= 1_000_000 {
		return "\(sign)\(formatOperatorCompactDecimal(absoluteValue / 1_000_000))M"
	}
	if absoluteValue >= 1_000 {
		return "\(sign)\(formatOperatorCompactDecimal(absoluteValue / 1_000))k"
	}

	return "\(value)"
}

private func formatOperatorCompactDecimal(_ value: Double) -> String {
	if value >= 100 {
		return String(format: "%.0f", value)
	}
	if value >= 10 {
		return String(format: "%.1f", value)
	}

	return String(format: "%.2f", value)
}

private func formatOperatorCompactBytes(_ value: Int) -> String {
	let units = ["B", "KiB", "MiB", "GiB"]
	var amount = Double(max(0, value))
	var unitIndex = 0
	while amount >= 1024, unitIndex < units.count - 1 {
		amount /= 1024
		unitIndex += 1
	}

	if unitIndex == 0 {
		return "\(Int(amount))\(units[unitIndex])"
	}
	if amount >= 100 {
		return "\(Int(amount.rounded()))\(units[unitIndex])"
	}

	return String(format: "%.1f%@", amount, units[unitIndex])
}

private func date(fromUnixEpoch value: Int64?) -> Date? {
	guard let value else {
		return nil
	}

	return Date(timeIntervalSince1970: TimeInterval(value))
}
