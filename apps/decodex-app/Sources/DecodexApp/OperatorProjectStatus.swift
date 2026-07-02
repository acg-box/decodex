import Foundation

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
