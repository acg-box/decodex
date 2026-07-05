use crate::orchestrator::execution_architecture_recovery::{
	ARCHITECTURE_RECOVERY_BUDGET, AuthorityBoundaryPolicyDecision, LoopGuardrailStopRequested,
	events::decision_request,
};

pub(crate) fn architecture_recovery_goal_detail(
	stop: &LoopGuardrailStopRequested,
	recovery_attempt_number: usize,
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> String {
	format!(
		"Loop guardrail `{}` stopped the current ineffective strategy after {} matching observations. Decodex recorded an Architecture Recovery Packet and an Authority Boundary Check with policy `{}`; use autonomous architecture recovery attempt {} of {}. Start a materially different implementation strategy, preserve the accepted Decision Contract and all validation/review gates, and {}.",
		stop.reason.error_class(),
		stop.consecutive_count,
		policy_decision.as_str(),
		recovery_attempt_number,
		ARCHITECTURE_RECOVERY_BUDGET,
		decision_request::architecture_recovery_policy_recovery_guidance(policy_decision)
	)
}

pub(crate) fn architecture_recovery_retry_next_action(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue =>
			"decodex recorded authority policy `auto_continue` and will retry with a materially different architecture recovery strategy",
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence =>
			"decodex recorded authority policy `requires_enhanced_evidence` and will retry with a materially different architecture recovery strategy while preserving enhanced evidence before review handoff or landing",
		AuthorityBoundaryPolicyDecision::BlockLanding =>
			"decodex recorded authority policy `block_landing` and will retry with a materially different architecture recovery strategy while landing remains blocked until validation or review-policy evidence is restored",
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision =>
			"decodex recorded authority policy `requires_human_decision` and requires human attention before retrying",
	}
}
