use std::collections::BTreeMap;

use crate::orchestrator::harness_improvement::{
	self, HARNESS_OUTCOME_EVENT_TYPE, HarnessImprovementCandidateSummary,
	HarnessLinearProjectionSummary, HarnessOutcomeKind, HarnessOutcomeRecordInput, IssueRunPlan,
	PrivateExecutionEvent, Result, StateStore,
	candidates::{self},
	payload,
};

pub(crate) fn record_harness_outcome_for_issue_run(
	state_store: &StateStore,
	input: HarnessOutcomeRecordInput<'_>,
) -> Result<PrivateExecutionEvent> {
	let events = state_store.list_private_execution_events(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
	)?;
	let contracts = harness_improvement::harness_contracts_for_issue(state_store, &input)?;
	let programs = harness_improvement::harness_programs_for_contracts(
		state_store,
		input.project_id,
		&contracts,
	)?;
	let linear_records =
		state_store.list_linear_execution_events(input.project_id, input.issue_id)?;
	let signals = payload::harness_outcome_signals(&events, input.outcome, input.error_class);
	let payload = harness_improvement::harness_outcome_payload(
		&input,
		&contracts,
		&programs,
		&linear_records,
		&signals,
	)?;

	state_store.append_private_execution_event(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
		HARNESS_OUTCOME_EVENT_TYPE,
		payload,
	)
}

pub(crate) fn harness_improvement_candidates_from_private_events(
	events: &[PrivateExecutionEvent],
) -> Vec<HarnessImprovementCandidateSummary> {
	let mut from_outcome = Vec::new();

	for event in events.iter().filter(|event| event.event_type() == HARNESS_OUTCOME_EVENT_TYPE) {
		from_outcome.extend(candidates::harness_candidates_from_payload(event.payload()));
	}

	if !from_outcome.is_empty() {
		return from_outcome;
	}
	if events.is_empty() {
		return Vec::new();
	}

	let signals =
		payload::harness_outcome_signals(events, HarnessOutcomeKind::TerminalFailure, None);

	if signals.validation_failure_count == 0
		&& signals.accepted_finding_count == 0
		&& signals.guardrail_reasons.is_empty()
		&& signals.authority_boundary_candidates.is_empty()
	{
		return Vec::new();
	}

	let input = HarnessOutcomeRecordInput {
		project_id: "",
		issue_id: "",
		issue_identifier: "local-readback",
		run_id: "",
		attempt_number: 0,
		outcome: HarnessOutcomeKind::TerminalFailure,
		error_class: None,
		validation_result: None,
		pr_url: None,
	};
	let linear_projection = HarnessLinearProjectionSummary {
		event_types: Vec::new(),
		final_event_type: None,
		final_error_class: None,
		final_terminal_path: None,
	};
	let mut candidates = BTreeMap::new();

	candidates::push_signal_candidates(&mut candidates, &input, &signals, &linear_projection);

	candidates.into_values().collect()
}

pub(crate) fn record_harness_outcome_best_effort(
	state_store: &StateStore,
	project_id: &str,
	issue_run: &IssueRunPlan,
	outcome: HarnessOutcomeKind,
	error_class: Option<&str>,
	validation_result: Option<&str>,
	pr_url: Option<&str>,
) {
	if let Err(error) = record_harness_outcome_for_issue_run(
		state_store,
		HarnessOutcomeRecordInput {
			project_id,
			issue_id: &issue_run.issue.id,
			issue_identifier: &issue_run.issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
			outcome,
			error_class,
			validation_result,
			pr_url,
		},
	) {
		tracing::warn!(
			?error,
			project_id,
			issue_id = issue_run.issue.id,
			issue = issue_run.issue.identifier,
			run_id = issue_run.run_id,
			attempt = issue_run.attempt_number,
			"Harness outcome telemetry write failed."
		);
	}
}
