use crate::orchestrator::execution_architecture_recovery::{
	self, ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_PACKET_SCHEMA, ArchitectureRecoveryPacketInput,
	LOOP_GUARDRAIL_CONVERGENCE_BUDGET, Result, StateStore, events::payloads,
};

pub(crate) fn record_architecture_recovery_packet(
	state_store: &StateStore,
	input: ArchitectureRecoveryPacketInput<'_>,
) -> Result<()> {
	let programs = payloads::architecture_recovery_programs_for_contracts(
		state_store,
		input.project.service_id(),
		input.contracts,
	)?;
	let retained =
		payloads::architecture_recovery_retained_worktree(&input.issue_run.worktree.path)?;
	let review = payloads::architecture_recovery_review_findings(
		state_store,
		input.project,
		input.issue_run,
	)?;

	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
			execution_architecture_recovery::json!({
				"schema": ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
				"record_version": 1,
				"state": input.reason_code,
				"reason_code": input.reason_code,
				"issue": payloads::architecture_recovery_issue_payload(input.issue_run),
				"run": payloads::architecture_recovery_run_payload(input.issue_run),
				"decision_contract_context": input.contracts
					.iter()
					.map(payloads::architecture_recovery_contract_payload)
					.collect::<Vec<_>>(),
				"execution_program_context": programs
					.iter()
					.map(payloads::architecture_recovery_program_payload)
					.collect::<Vec<_>>(),
				"retained_worktree": retained,
				"validation_failures": payloads::architecture_recovery_validation_failures(
					input.loop_guardrail_stop,
					input.error,
				),
				"review_findings": review,
				"prior_recovery_attempts": {
					"started_count": input.prior_started_count,
				},
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"loop_guardrail": {
					"reason": input.loop_guardrail_stop.reason.error_class(),
					"consecutive_count": input.loop_guardrail_stop.consecutive_count,
					"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
					"fingerprint": input.loop_guardrail_stop.fingerprint.as_str(),
					"source_error_class": input.loop_guardrail_stop.source_error_class.as_deref(),
				},
				"authority_boundary_check": {
					"record_id": input.boundary_check_record_id,
					"disposition": input.boundary_disposition.as_str(),
					"policy_decision": input.boundary_policy_decision.as_str(),
					"requires_enhanced_evidence": input
						.boundary_policy_decision
						.requires_enhanced_evidence(),
					"blocks_landing": input.boundary_policy_decision.blocks_landing(),
					"reason": input.boundary_final_reason,
				},
			}),
		)
		.map(|_| ())
}
