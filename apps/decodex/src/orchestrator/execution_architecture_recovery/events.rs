mod decision_request;
mod payloads;

use crate::orchestrator::execution_architecture_recovery::{
	self, ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_PACKET_SCHEMA, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, ArchitectureRecoveryPacketInput,
	ArchitectureRecoveryTerminalEventInput, AuthorityBoundaryPolicyDecision, IssueRunPlan,
	LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailStopRequested, Result, ServiceConfig,
	StateStore,
};
use payloads::{
	architecture_recovery_contract_payload, architecture_recovery_issue_payload,
	architecture_recovery_program_payload, architecture_recovery_run_payload,
	architecture_recovery_validation_failures,
};

pub(super) fn record_architecture_recovery_packet(
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
				"issue": architecture_recovery_issue_payload(input.issue_run),
				"run": architecture_recovery_run_payload(input.issue_run),
				"decision_contract_context": input.contracts
					.iter()
					.map(architecture_recovery_contract_payload)
					.collect::<Vec<_>>(),
				"execution_program_context": programs
					.iter()
					.map(architecture_recovery_program_payload)
					.collect::<Vec<_>>(),
				"retained_worktree": retained,
				"validation_failures": architecture_recovery_validation_failures(
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

pub(super) fn record_architecture_recovery_started_event(
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

pub(super) fn record_architecture_recovery_terminal_outcome(
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

pub(super) fn architecture_recovery_goal_detail(
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
