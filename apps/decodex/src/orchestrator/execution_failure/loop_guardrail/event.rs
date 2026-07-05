use crate::orchestrator::execution_failure::{
	self, IssueRunPlan, LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailCheckpoint, Result,
	ServiceConfig, StateStore,
};

pub(in crate::orchestrator::execution_failure::loop_guardrail) fn record_loop_guardrail_private_event(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	checkpoint: &LoopGuardrailCheckpoint,
	source_error_class: Option<&str>,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"loop_guardrail_checkpoint",
			execution_failure::json!({
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"reason": checkpoint.reason(),
				"fingerprint": checkpoint.fingerprint(),
				"consecutive_count": checkpoint.consecutive_count(),
				"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
				"checkpoint_run_id": checkpoint.run_id(),
				"checkpoint_attempt_number": checkpoint.attempt_number(),
				"source_error_class": source_error_class,
				"details": checkpoint.details_json(),
			}),
		)
		.map(|_| ())
}
