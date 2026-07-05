use crate::orchestrator::execution_failure::{
	self, LoopGuardrailReason, LoopGuardrailStopRequested, Report, ReviewPolicyStopRequested,
};

pub(crate) fn loop_guardrail_stop_from_review_policy(
	review_policy_stop: &ReviewPolicyStopRequested,
) -> LoopGuardrailStopRequested {
	LoopGuardrailStopRequested {
		issue_identifier: review_policy_stop.issue_identifier.clone(),
		run_id: review_policy_stop.run_id.clone(),
		reason: LoopGuardrailReason::ReviewChurn,
		consecutive_count: review_policy_stop.nonclean_rounds.unwrap_or_default(),
		fingerprint: review_policy_stop.fingerprint.clone().unwrap_or_else(|| {
			format!(
				"{}:{}",
				review_policy_stop.head_sha,
				review_policy_stop.nonclean_rounds.unwrap_or_default()
			)
		}),
		source_error_class: Some(review_policy_stop.reason.error_class().to_owned()),
		architecture_recovery_reason_code: None,
	}
}

pub(crate) fn run_failure_requires_terminal_attention(error: &Report) -> bool {
	execution_failure::run_failure_writeback_disposition(error).requires_terminal_attention()
}
