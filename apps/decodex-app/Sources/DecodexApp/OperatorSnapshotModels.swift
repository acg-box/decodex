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
