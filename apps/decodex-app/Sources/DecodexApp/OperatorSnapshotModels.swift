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
		let labels = warnings.compactMap(warningLabel).filter { $0.isEmpty == false }
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
		activeRuns(for: account).count
	}

	func mergingRunActivity(_ activityRuns: [OperatorRunStatus]) -> OperatorSnapshotResponse {
		let snapshotRunsByID = activeRuns.reduce(into: [String: OperatorRunStatus]()) { runsByID, run in
			runsByID[run.runID] = run
		}
		let mergedRuns = activityRuns.map { activityRun in
			snapshotRunsByID[activityRun.runID]?.mergingActivity(activityRun) ?? activityRun
		}
		let activeCountsByProject = Dictionary(grouping: mergedRuns.compactMap(\.projectID)) { $0 }
			.mapValues(\.count)
		let mergedProjects = projects.map { project in
			guard let projectID = project.projectID else {
				return project
			}

			return project.withActiveRunCount(activeCountsByProject[projectID] ?? 0)
		}

		return OperatorSnapshotResponse(
			warnings: warnings,
			projects: mergedProjects,
			activeRuns: mergedRuns,
			queuedCandidates: queuedCandidates,
			postReviewLanes: postReviewLanes
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

	private var snapshotBuildFailureProjectIDs: [String] {
		let apiOnlyBaseline = warnings.contains("automation_disabled")

		return projects.compactMap { project in
			guard project.enabled, let projectID = project.projectID, projectID.isEmpty == false else {
				return nil
			}
			if project.connectorState == "config_error" {
				return projectID
			}
			if apiOnlyBaseline && project.warningCount > 1 {
				return projectID
			}
			if project.connectorState == "degraded" && project.warningCount > 0 {
				return projectID
			}

			return nil
		}
	}

	private func snapshotBuildFailureLabel() -> String {
		let projectIDs = snapshotBuildFailureProjectIDs
		guard let first = projectIDs.first else {
			return "Snapshot build failed"
		}
		if projectIDs.count == 1 {
			return "Snapshot build failed: \(first)"
		}

		return "Snapshot build failed: \(first) +\(projectIDs.count - 1)"
	}

	private func warningLabel(_ value: String) -> String? {
		switch value {
		case "automation_disabled":
			return nil
		case "control_plane_tick_context_failed":
			return "Control-plane context unavailable"
		case "operator_snapshot_build_failed":
			return snapshotBuildFailureLabel()
		case "control_plane_tick_failed":
			return "Control-plane tick failed"
		case "tracker_rate_limited":
			return "Tracker sync paused"
		case "codex_accounts_unavailable":
			return "Accounts unavailable"
		case "worktree_hygiene_unavailable":
			return "Worktree hygiene unavailable"
		default:
			return readable(value)
		}
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
	let queuedCandidateCount: Int
	let postReviewLaneCount: Int
	let waitingLaneCount: Int
	let attentionCount: Int
	let cleanupBlockedCount: Int
	let cleanupPendingCount: Int

	func withActiveRunCount(_ count: Int) -> OperatorProjectStatus {
		OperatorProjectStatus(
			projectID: projectID,
			enabled: enabled,
			connectorState: connectorState,
			warningCount: warningCount,
			activeRunCount: count,
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
			return readable(currentBucket)
		}
		if let waitReason = trimmed(waitReason), waitReason.isEmpty == false {
			return readable(waitReason)
		}
		if let operation = trimmed(currentOperation), operation.isEmpty == false, operation != "idle" {
			return readable(operation)
		}
		if let phase = trimmed(phase), phase.isEmpty == false {
			return readable(phase)
		}
		if let threadStatus = trimmed(threadStatus), threadStatus.isEmpty == false {
			return readable(threadStatus)
		}
		if let status = trimmed(status), status.isEmpty == false {
			return readable(status)
		}

		return "Active"
	}

	var hasAttentionTone: Bool {
		suspectedStall
			|| attemptStatus == "waiting_for_review"
			|| status == "manual_attention"
			|| status == "blocked"
	}

	var isWaiting: Bool {
		waitReason != nil
			|| attemptStatus?.contains("waiting") == true
			|| phase?.contains("waiting") == true
	}

	func isAssigned(to account: CodexAccount) -> Bool {
		self.account?.matches(account) == true
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

		return title == readable(currentOperation ?? phase ?? "")
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
		case account
		case accounts
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
		account = try container.decodeIfPresent(OperatorRunAccountSummary.self, forKey: .account)
		accounts = try container.decodeIfPresent([OperatorRunAccountSummary].self, forKey: .accounts) ?? []
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
	}
}

struct OperatorChildAgentActivity: Decodable, Sendable {
	let currentBucket: String?
	let currentDetail: String?
	let currentElapsedSeconds: Int?
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
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		accountFingerprint = try container.decodeIfPresent(String.self, forKey: .accountFingerprint) ?? ""
		email = try container.decodeIfPresent(String.self, forKey: .email)
	}
}

private func trimmed(_ value: String?) -> String? {
	value?.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func readable(_ value: String) -> String {
	let words = value
		.replacingOccurrences(of: "-", with: " ")
		.replacingOccurrences(of: "_", with: " ")
		.split(separator: " ")
		.map { word in
			let text = String(word)
			guard let first = text.first else {
				return text
			}

			return first.uppercased() + String(text.dropFirst())
		}

	return words.joined(separator: " ")
}

private func date(fromUnixEpoch value: Int64?) -> Date? {
	guard let value else {
		return nil
	}

	return Date(timeIntervalSince1970: TimeInterval(value))
}
