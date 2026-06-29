import Foundation

struct OperatorContinuationRecoveryStatus: Decodable, Sendable {
	let state: String?
	let sourcePhase: String?
	let nextPhase: String?
	let sourceErrorClass: String?
	let sourceErrorMessage: String?
	let recordedAt: String?
	let runID: String?
	let attemptNumber: Int?
	let recoveryCount: Int?
	let automaticContinuationLimit: Int?
	let budgetExceeded: Bool?
	let nextAction: String?

	enum CodingKeys: String, CodingKey {
		case state
		case sourcePhase = "source_phase"
		case nextPhase = "next_phase"
		case sourceErrorClass = "source_error_class"
		case sourceErrorMessage = "source_error_message"
		case recordedAt = "recorded_at"
		case runID = "run_id"
		case attemptNumber = "attempt_number"
		case recoveryCount = "recovery_count"
		case automaticContinuationLimit = "automatic_continuation_limit"
		case budgetExceeded = "budget_exceeded"
		case nextAction = "next_action"
	}
}
