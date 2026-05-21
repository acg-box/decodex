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
			queuedCandidates.filter { !$0.isClosed }.count,
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
			|| !warnings.isEmpty
	}

	var shouldDisplayInPanel: Bool {
		!isAPIOnlySnapshot
	}

	var warningSummary: String? {
		let labels = warnings.compactMap(Self.warningLabel).filter { !$0.isEmpty }
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

	private var isAPIOnlySnapshot: Bool {
		warnings.contains("automation_disabled")
			&& projects.allSatisfy { $0.connectorState == "api_only" }
	}

	private static func warningLabel(_ value: String) -> String? {
		switch value {
		case "automation_disabled":
			return nil
		case "control_plane_tick_context_failed":
			return "Control-plane context unavailable"
		case "operator_snapshot_build_failed":
			return "Snapshot build failed"
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
	let connectorState: String?
	let activeRunCount: Int
	let queuedCandidateCount: Int
	let postReviewLaneCount: Int
	let waitingLaneCount: Int
	let attentionCount: Int
	let cleanupBlockedCount: Int
	let cleanupPendingCount: Int

	enum CodingKeys: String, CodingKey {
		case connectorState = "connector_state"
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

		connectorState = try container.decodeIfPresent(String.self, forKey: .connectorState)
		activeRunCount = try container.decodeIfPresent(Int.self, forKey: .activeRunCount) ?? 0
		queuedCandidateCount = try container.decodeIfPresent(Int.self, forKey: .queuedCandidateCount) ?? 0
		postReviewLaneCount = try container.decodeIfPresent(Int.self, forKey: .postReviewLaneCount) ?? 0
		waitingLaneCount = try container.decodeIfPresent(Int.self, forKey: .waitingLaneCount) ?? 0
		attentionCount = try container.decodeIfPresent(Int.self, forKey: .attentionCount) ?? 0
		cleanupBlockedCount = try container.decodeIfPresent(Int.self, forKey: .cleanupBlockedCount) ?? 0
		cleanupPendingCount = try container.decodeIfPresent(Int.self, forKey: .cleanupPendingCount) ?? 0
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
	let runID: String
	let issueIdentifier: String?
	let title: String?
	let status: String?
	let attemptStatus: String?
	let phase: String?
	let waitReason: String?
	let currentOperation: String?
	let threadStatus: String?
	let idleForSeconds: Int?
	let suspectedStall: Bool
	let childAgentActivity: OperatorChildAgentActivity?
	let account: OperatorRunAccountSummary?
	let accounts: [OperatorRunAccountSummary]

	var id: String {
		runID
	}

	var compactTitle: String {
		if let issueIdentifier = trimmed(issueIdentifier), !issueIdentifier.isEmpty {
			return issueIdentifier
		}
		if let title = trimmed(title), !title.isEmpty {
			return title
		}

		return "Run"
	}

	var compactDetail: String {
		if let currentDetail = trimmed(childAgentActivity?.currentDetail), !currentDetail.isEmpty {
			return currentDetail
		}
		if let currentBucket = trimmed(childAgentActivity?.currentBucket), !currentBucket.isEmpty {
			return readable(currentBucket)
		}
		if let waitReason = trimmed(waitReason), !waitReason.isEmpty {
			return readable(waitReason)
		}
		if let operation = trimmed(currentOperation), !operation.isEmpty, operation != "idle" {
			return readable(operation)
		}
		if let phase = trimmed(phase), !phase.isEmpty {
			return readable(phase)
		}
		if let threadStatus = trimmed(threadStatus), !threadStatus.isEmpty {
			return readable(threadStatus)
		}
		if let status = trimmed(status), !status.isEmpty {
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
		let runAccounts = ([self.account].compactMap { $0 }) + accounts

		return runAccounts.contains { $0.matches(account) }
	}

	enum CodingKeys: String, CodingKey {
		case runID = "run_id"
		case issueIdentifier = "issue_identifier"
		case title
		case status
		case attemptStatus = "attempt_status"
		case phase
		case waitReason = "wait_reason"
		case currentOperation = "current_operation"
		case threadStatus = "thread_status"
		case idleForSeconds = "idle_for_seconds"
		case suspectedStall = "suspected_stall"
		case childAgentActivity = "child_agent_activity"
		case account
		case accounts
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		runID = try container.decodeIfPresent(String.self, forKey: .runID) ?? UUID().uuidString
		issueIdentifier = try container.decodeIfPresent(String.self, forKey: .issueIdentifier)
		title = try container.decodeIfPresent(String.self, forKey: .title)
		status = try container.decodeIfPresent(String.self, forKey: .status)
		attemptStatus = try container.decodeIfPresent(String.self, forKey: .attemptStatus)
		phase = try container.decodeIfPresent(String.self, forKey: .phase)
		waitReason = try container.decodeIfPresent(String.self, forKey: .waitReason)
		currentOperation = try container.decodeIfPresent(String.self, forKey: .currentOperation)
		threadStatus = try container.decodeIfPresent(String.self, forKey: .threadStatus)
		idleForSeconds = try container.decodeIfPresent(Int.self, forKey: .idleForSeconds)
		suspectedStall = try container.decodeIfPresent(Bool.self, forKey: .suspectedStall) ?? false
		childAgentActivity = try container.decodeIfPresent(
			OperatorChildAgentActivity.self,
			forKey: .childAgentActivity
		)
		account = try container.decodeIfPresent(OperatorRunAccountSummary.self, forKey: .account)
		accounts = try container.decodeIfPresent([OperatorRunAccountSummary].self, forKey: .accounts) ?? []
	}
}

struct OperatorChildAgentActivity: Decodable, Sendable {
	let currentBucket: String?
	let currentDetail: String?
	let toolCallCount: Int

	enum CodingKeys: String, CodingKey {
		case currentBucket = "current_bucket"
		case currentDetail = "current_detail"
		case toolCallCount = "tool_call_count"
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: CodingKeys.self)

		currentBucket = try container.decodeIfPresent(String.self, forKey: .currentBucket)
		currentDetail = try container.decodeIfPresent(String.self, forKey: .currentDetail)
		toolCallCount = try container.decodeIfPresent(Int.self, forKey: .toolCallCount) ?? 0
	}
}

struct OperatorRunAccountSummary: Decodable, Sendable {
	let accountFingerprint: String
	let email: String?

	func matches(_ account: CodexAccount) -> Bool {
		if !accountFingerprint.isEmpty, accountFingerprint == account.accountFingerprint {
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
