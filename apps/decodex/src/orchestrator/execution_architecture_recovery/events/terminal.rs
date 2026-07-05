use crate::orchestrator::execution_architecture_recovery::{
	self, ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
	ArchitectureRecoveryTerminalEventInput, Result, StateStore, events::decision_request,
};

pub(crate) fn record_architecture_recovery_terminal_outcome(
	state_store: &StateStore,
	input: ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	record_architecture_recovery_terminal_event(state_store, &input)?;

	if input.boundary_policy_decision.allows_autonomous_recovery() {
		return Ok(());
	}

	let decision_request_id = format!(
		"{}-{}-{}-{}",
		input.issue_run.issue.identifier,
		input.issue_run.run_id,
		input.issue_run.attempt_number,
		input.reason_code
	);

	execution_architecture_recovery::record_authority_decision_request_private_event(
		state_store,
		decision_request::architecture_recovery_decision_request_input(
			input.project,
			input.issue_run,
			input.stop,
			input.boundary_check_record_id,
			&decision_request_id,
			input.reason_code,
			input.boundary_final_reason,
		),
	)
	.map(|_| ())
}

fn record_architecture_recovery_terminal_event(
	state_store: &StateStore,
	input: &ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
			execution_architecture_recovery::json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"record_version": 1,
				"reason_code": input.reason_code,
				"guardrail_reason": input.stop.reason.error_class(),
				"authority_boundary_check_record_id": input.boundary_check_record_id,
				"boundary_disposition": input.boundary_disposition.as_str(),
				"boundary_policy_decision": input.boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": input
					.boundary_policy_decision
					.requires_enhanced_evidence(),
				"blocks_landing": input.boundary_policy_decision.blocks_landing(),
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
			}),
		)
		.map(|_| ())
}
