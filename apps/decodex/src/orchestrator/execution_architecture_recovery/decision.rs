use crate::orchestrator::execution_architecture_recovery::{
	self, ARCHITECTURE_RECOVERY_BUDGET, ArchitectureRecoveryPacketInput, ArchitectureRecoveryStart,
	ArchitectureRecoveryTerminalEventInput, AuthorityBoundaryCheckInput,
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, IssueRunPlan,
	LoopGuardrailRecoveryDecision, LoopGuardrailStopRequested, Report, Result, ServiceConfig,
	StateStore,
};

struct ArchitectureRecoveryDecisionEvents<'a> {
	boundary_check_record_id: i64,
	disposition: AuthorityBoundaryDisposition,
	policy_decision: AuthorityBoundaryPolicyDecision,
	final_reason: &'a str,
	reason_code: &'static str,
	recovery_attempt_number: usize,
}

pub(crate) fn loop_guardrail_architecture_recovery_decision(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	mut loop_guardrail_stop: LoopGuardrailStopRequested,
	error: &Report,
) -> Result<LoopGuardrailRecoveryDecision> {
	let prior_started_count = execution_architecture_recovery::architecture_recovery_started_count(
		state_store,
		project,
		issue_run,
	)?;
	let recovery_attempt_number = prior_started_count.saturating_add(1);
	let boundary = execution_architecture_recovery::classify_loop_guardrail_authority_boundary(
		&loop_guardrail_stop,
		error,
	);
	let changed_surfaces = execution_architecture_recovery::architecture_recovery_changed_surfaces(
		&boundary,
		&issue_run.worktree.path,
	);
	let policy_decision =
		execution_architecture_recovery::architecture_recovery_policy_decision(&changed_surfaces);
	let disposition = policy_decision.disposition();
	let final_reason = execution_architecture_recovery::architecture_recovery_final_reason(
		&boundary,
		policy_decision,
	);
	let contracts = execution_architecture_recovery::architecture_recovery_contracts_for_issue(
		state_store,
		project,
		issue_run,
	)?;
	let decision_contract_ids =
		contracts.iter().map(|contract| contract.contract_id().to_owned()).collect::<Vec<_>>();
	let decision_contract_id_refs =
		decision_contract_ids.iter().map(String::as_str).collect::<Vec<_>>();
	let boundary_event =
		execution_architecture_recovery::record_authority_boundary_check_private_event(
			state_store,
			AuthorityBoundaryCheckInput {
				project_id: project.service_id(),
				issue_id: &issue_run.issue.id,
				issue_identifier: &issue_run.issue.identifier,
				run_id: &issue_run.run_id,
				attempt_number: issue_run.attempt_number,
				decision_contract_ids: decision_contract_id_refs,
				attempted_recovery_reason: loop_guardrail_stop.reason.error_class(),
				changed_surfaces,
				policy_decision,
				disposition,
				final_disposition_reason: final_reason,
				improvement_signals:
					execution_architecture_recovery::architecture_recovery_improvement_signals(
						loop_guardrail_stop.reason,
						&boundary,
					),
			},
		)?;
	let budget_exhausted = prior_started_count >= ARCHITECTURE_RECOVERY_BUDGET;
	let reason_code = execution_architecture_recovery::architecture_recovery_reason_code(
		&boundary,
		policy_decision,
		budget_exhausted,
	);

	execution_architecture_recovery::record_architecture_recovery_packet(
		state_store,
		ArchitectureRecoveryPacketInput {
			project,
			issue_run,
			loop_guardrail_stop: &loop_guardrail_stop,
			error,
			contracts: &contracts,
			boundary_check_record_id: boundary_event.record_id(),
			boundary_disposition: disposition,
			boundary_policy_decision: policy_decision,
			boundary_final_reason: final_reason,
			reason_code,
			recovery_attempt_number,
			prior_started_count,
		},
	)?;

	if budget_exhausted || !policy_decision.allows_autonomous_recovery() {
		loop_guardrail_stop.architecture_recovery_reason_code = Some(reason_code.to_owned());

		return record_terminal_architecture_recovery_decision(
			state_store,
			project,
			issue_run,
			loop_guardrail_stop,
			ArchitectureRecoveryDecisionEvents {
				boundary_check_record_id: boundary_event.record_id(),
				disposition,
				policy_decision,
				final_reason,
				reason_code,
				recovery_attempt_number,
			},
		);
	}

	record_started_architecture_recovery_decision(
		state_store,
		project,
		issue_run,
		&loop_guardrail_stop,
		ArchitectureRecoveryDecisionEvents {
			boundary_check_record_id: boundary_event.record_id(),
			disposition,
			policy_decision,
			final_reason,
			reason_code,
			recovery_attempt_number,
		},
	)
}

fn record_terminal_architecture_recovery_decision(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	loop_guardrail_stop: LoopGuardrailStopRequested,
	events: ArchitectureRecoveryDecisionEvents<'_>,
) -> Result<LoopGuardrailRecoveryDecision> {
	execution_architecture_recovery::record_architecture_recovery_terminal_outcome(
		state_store,
		ArchitectureRecoveryTerminalEventInput {
			project,
			issue_run,
			stop: &loop_guardrail_stop,
			boundary_check_record_id: events.boundary_check_record_id,
			boundary_disposition: events.disposition,
			boundary_policy_decision: events.policy_decision,
			boundary_final_reason: events.final_reason,
			reason_code: events.reason_code,
			recovery_attempt_number: events.recovery_attempt_number,
		},
	)?;

	Ok(LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop))
}

fn record_started_architecture_recovery_decision(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	loop_guardrail_stop: &LoopGuardrailStopRequested,
	events: ArchitectureRecoveryDecisionEvents<'_>,
) -> Result<LoopGuardrailRecoveryDecision> {
	state_store.clear_loop_guardrail_checkpoint(
		project.service_id(),
		&issue_run.issue.id,
		loop_guardrail_stop.reason.error_class(),
	)?;

	execution_architecture_recovery::record_architecture_recovery_started_event(
		state_store,
		project,
		issue_run,
		loop_guardrail_stop,
		events.boundary_check_record_id,
		events.policy_decision,
		events.recovery_attempt_number,
	)?;

	Ok(LoopGuardrailRecoveryDecision::Start(ArchitectureRecoveryStart {
		attempt_number: events.recovery_attempt_number,
		max_attempts: ARCHITECTURE_RECOVERY_BUDGET,
		policy_decision: events.policy_decision,
		detail: execution_architecture_recovery::architecture_recovery_goal_detail(
			loop_guardrail_stop,
			events.recovery_attempt_number,
			events.policy_decision,
		),
	}))
}
