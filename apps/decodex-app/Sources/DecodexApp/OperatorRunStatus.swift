import Foundation

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
}
