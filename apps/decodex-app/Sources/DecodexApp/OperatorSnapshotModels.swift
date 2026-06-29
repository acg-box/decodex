import Foundation

struct OperatorSnapshotResponse: Decodable, Sendable {
	let warnings: [String]
	let projects: [OperatorProjectStatus]
	let currentLanes: [OperatorRunStatus]
	let presentation: OperatorSnapshotPresentation?
	let queuedCandidates: [OperatorQueuedIssueStatus]
	let postReviewLanes: [OperatorPostReviewLaneStatus]

	var currentLaneCount: Int {
		max(
			currentLanes.count,
			projects.reduce(0) { $0 + $1.currentLaneCount }
		)
	}

	var runningLaneCount: Int {
		max(
			currentLanes.filter(\.countsAsRunning).count,
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
			postReviewLanes.filter { $0.shadowedByCurrentLane == false }.count,
			projects.reduce(0) { $0 + $1.postReviewLaneCount }
		)
	}

	var landingCount: Int {
		postReviewLanes.filter { $0.isReadyToLand && $0.shadowedByCurrentLane == false }.count
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
		currentLaneCount > 0
			|| queuedCount > 0
			|| waitingCount > 0
			|| attentionCount > 0
			|| cleanupCount > 0
			|| warnings.isEmpty == false
	}

	var shouldDisplayInPanel: Bool {
		hasVisibleSignal && (currentLaneCount > 0 || isDevSnapshot == false)
	}

	var warningSummary: String? {
		let labels = warnings
			.filter { $0 != "automation_disabled" }
			.map(operatorRawDisplayToken)
			.filter { $0.isEmpty == false }
		guard let first = labels.first else {
			return nil
		}
		if labels.count == 1 {
			return first
		}

		return "\(first) +\(labels.count - 1)"
	}

	func currentLanes(for account: CodexAccount) -> [OperatorRunStatus] {
		currentLanes.filter { $0.isAssigned(to: account) }
	}

	func runningCount(for account: CodexAccount) -> Int {
		currentLanes(for: account).filter(\.countsAsRunning).count
	}

	private init(
		warnings: [String],
		projects: [OperatorProjectStatus],
		currentLanes: [OperatorRunStatus],
		presentation: OperatorSnapshotPresentation?,
		queuedCandidates: [OperatorQueuedIssueStatus],
		postReviewLanes: [OperatorPostReviewLaneStatus]
	) {
		self.warnings = warnings
		self.projects = projects
		self.currentLanes = currentLanes
		self.presentation = presentation
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
		case currentLanes = "current_lanes"
		case presentation
		case queuedCandidates = "queued_candidates"
		case postReviewLanes = "post_review_lanes"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		warnings = try container.decodeIfPresent([String].self, forKey: .warnings) ?? []
		projects = try container.decodeIfPresent([OperatorProjectStatus].self, forKey: .projects) ?? []
		currentLanes = try container.decodeIfPresent([OperatorRunStatus].self, forKey: .currentLanes) ?? []
		presentation = try container.decodeIfPresent(OperatorSnapshotPresentation.self, forKey: .presentation)
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
	let currentLaneCount: Int
	let runningLaneCount: Int
	let queuedCandidateCount: Int
	let postReviewLaneCount: Int
	let waitingLaneCount: Int
	let attentionCount: Int
	let cleanupBlockedCount: Int
	let cleanupPendingCount: Int

	func withRunCounts(currentLanes: Int, running: Int) -> OperatorProjectStatus {
		OperatorProjectStatus(
			projectID: projectID,
			enabled: enabled,
			connectorState: connectorState,
			warningCount: warningCount,
			currentLaneCount: currentLanes,
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
		case currentLaneCount = "current_lane_count"
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
		currentLaneCount = try container.decodeIfPresent(Int.self, forKey: .currentLaneCount) ?? 0
		runningLaneCount =
			try container.decodeIfPresent(Int.self, forKey: .runningLaneCount) ?? currentLaneCount
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
		currentLaneCount: Int,
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
		self.currentLaneCount = currentLaneCount
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
	let shadowedByCurrentLane: Bool

	var isReadyToLand: Bool {
		classification == "ready_to_land" && shadowedByCurrentLane == false
	}

	enum CodingKeys: String, CodingKey {
		case classification
		case shadowedByCurrentLane = "shadowed_by_current_lane"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)
		classification = try container.decodeIfPresent(String.self, forKey: .classification)
		shadowedByCurrentLane = try container.decode(Bool.self, forKey: .shadowedByCurrentLane)
	}
}
