use crate::orchestrator::execution_architecture_recovery::{
	self, ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	AuthorityBoundaryPolicyDecision, IssueRunPlan, LoopGuardrailStopRequested, Result,
	ServiceConfig, StateStore,
};

pub(crate) fn record_architecture_recovery_started_event(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	stop: &LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	recovery_attempt_number: usize,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
			execution_architecture_recovery::json!({
				"schema": "decodex.architecture_recovery_started/1",
				"record_version": 1,
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": stop.reason.error_class(),
				"authority_boundary_check_record_id": boundary_check_record_id,
				"boundary_policy_decision": boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": boundary_policy_decision.requires_enhanced_evidence(),
				"blocks_landing": boundary_policy_decision.blocks_landing(),
				"recovery_budget": {
					"attempt": recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"next_strategy": "materially_different_architecture_recovery",
			}),
		)
		.map(|_| ())
}
