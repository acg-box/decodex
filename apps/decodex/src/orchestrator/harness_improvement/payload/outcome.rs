use crate::orchestrator::harness_improvement::{
	self, DecisionContractRecord, ExecutionProgramRecord, HARNESS_OUTCOME_SCHEMA,
	HarnessAuthorityBoundaryOutcome, HarnessManualAttentionOutcome, HarnessOutcomeKind,
	HarnessOutcomePayload, HarnessOutcomeRecordInput, HarnessOutcomeSignals, HarnessOutcomeSource,
	HarnessPrLifecycleOutcome, HarnessRepairOutcome, HarnessReviewOutcome,
	HarnessValidationOutcome, LinearExecutionEventRecord, Result, Value,
	payload::{contracts, projection},
};

pub(in crate::orchestrator::harness_improvement) fn harness_outcome_payload(
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[DecisionContractRecord],
	programs: &[ExecutionProgramRecord],
	linear_records: &[LinearExecutionEventRecord],
	signals: &HarnessOutcomeSignals,
) -> Result<Value> {
	let source_intents = contracts.iter().map(contracts::harness_source_intent).collect();
	let contracts = contracts.iter().map(contracts::harness_outcome_contract).collect::<Vec<_>>();
	let programs = programs.iter().map(contracts::harness_outcome_program).collect::<Vec<_>>();
	let linear_projection = projection::harness_linear_projection(linear_records);
	let validation = HarnessValidationOutcome {
		result: input.outcome.validation_result(input.validation_result, signals),
		failure_count: signals.validation_failure_count,
		failure_classes: signals.validation_failure_classes.iter().cloned().collect(),
	};
	let repair = HarnessRepairOutcome {
		attempt_number: input.attempt_number,
		repair_attempt_observed: input.attempt_number > 1 || signals.repair_phase_events > 0,
		repair_phase_events: signals.repair_phase_events,
	};
	let review = HarnessReviewOutcome {
		statuses: signals.review_statuses.iter().cloned().collect(),
		accepted_finding_count: signals.accepted_finding_count,
		rejected_finding_count: signals.rejected_finding_count,
		nonclean_rounds: signals.nonclean_rounds,
	};
	let authority_boundary = HarnessAuthorityBoundaryOutcome {
		dispositions: signals.authority_boundary_dispositions.iter().cloned().collect(),
		failed_check_count: signals.authority_boundary_failed_check_count,
		improvement_signal_count: signals.authority_boundary_candidates.len(),
	};
	let manual_attention = input
		.error_class
		.filter(|_| input.outcome == HarnessOutcomeKind::ManualAttention)
		.map(|reason| HarnessManualAttentionOutcome { reason_code: reason.to_owned() });
	let pr_lifecycle = HarnessPrLifecycleOutcome {
		outcome: input.outcome.as_str().to_owned(),
		pr_urls: projection::harness_pr_urls(input.pr_url, linear_records),
	};
	let improvement_candidates = harness_improvement::harness_improvement_candidates(
		input,
		&contracts,
		&programs,
		signals,
		&linear_projection,
	);
	let payload = HarnessOutcomePayload {
		schema: HARNESS_OUTCOME_SCHEMA,
		record_version: 1,
		source: HarnessOutcomeSource {
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			issue_identifier: input.issue_identifier.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			outcome: input.outcome.as_str().to_owned(),
			source_intents,
		},
		contracts,
		execution_programs: programs,
		phase_goal_outcomes: signals.phase_goals.clone(),
		validation,
		repair,
		review,
		authority_boundary,
		manual_attention,
		pr_lifecycle,
		linear_projection,
		improvement_candidates,
	};

	serde_json::to_value(payload).map_err(Into::into)
}
