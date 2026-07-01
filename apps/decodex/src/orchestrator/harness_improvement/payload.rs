use super::{
	DecisionContractRecord, ExecutionConflictDomain, ExecutionProgramRecord,
	HARNESS_OUTCOME_SCHEMA, HarnessAuthorityBoundaryOutcome, HarnessImprovementCandidateSummary,
	HarnessLinearProjectionSummary, HarnessManualAttentionOutcome, HarnessOutcomeContract,
	HarnessOutcomeKind, HarnessOutcomePayload, HarnessOutcomeProgram, HarnessOutcomeProgramNode,
	HarnessOutcomeRecordInput, HarnessOutcomeSignals, HarnessOutcomeSource,
	HarnessPhaseGoalOutcome, HarnessPrLifecycleOutcome, HarnessRepairOutcome, HarnessReviewOutcome,
	HarnessSourceIntent, HarnessValidationOutcome, LinearExecutionEventRecord,
	PrivateExecutionEvent, Result, StateStore, Value,
	candidates::{
		authority_boundary_final_reason_mentions_underspecified, first_decision_contract_target,
		json_array_len, json_string,
	},
	harness_improvement_candidates,
};

pub(super) fn harness_contracts_for_issue(
	state_store: &StateStore,
	input: &HarnessOutcomeRecordInput<'_>,
) -> Result<Vec<DecisionContractRecord>> {
	let mut records = Vec::new();
	let mut seen = std::collections::BTreeSet::new();

	for issue_id in [input.issue_id, input.issue_identifier] {
		for record in state_store.list_decision_contracts_for_issue(input.project_id, issue_id)? {
			let key = record.contract_id().to_owned();

			if seen.insert(key) {
				records.push(record);
			}
		}
	}

	records.sort_by(|left, right| left.contract_id().cmp(right.contract_id()));

	Ok(records)
}

pub(super) fn harness_programs_for_contracts(
	state_store: &StateStore,
	project_id: &str,
	contracts: &[DecisionContractRecord],
) -> Result<Vec<ExecutionProgramRecord>> {
	let mut programs = Vec::new();
	let mut seen = std::collections::BTreeSet::new();

	for contract in contracts {
		for program in
			state_store.list_execution_programs_for_contract(project_id, contract.contract_id())?
		{
			let key = program.program_id().to_owned();

			if seen.insert(key) {
				programs.push(program);
			}
		}
	}

	programs.sort_by(|left, right| left.program_id().cmp(right.program_id()));

	Ok(programs)
}

pub(super) fn harness_outcome_payload(
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[DecisionContractRecord],
	programs: &[ExecutionProgramRecord],
	linear_records: &[LinearExecutionEventRecord],
	signals: &HarnessOutcomeSignals,
) -> Result<Value> {
	let source_intents = contracts.iter().map(harness_source_intent).collect();
	let contracts = contracts.iter().map(harness_outcome_contract).collect::<Vec<_>>();
	let programs = programs.iter().map(harness_outcome_program).collect::<Vec<_>>();
	let linear_projection = harness_linear_projection(linear_records);
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
		pr_urls: harness_pr_urls(input.pr_url, linear_records),
	};
	let improvement_candidates =
		harness_improvement_candidates(input, &contracts, &programs, signals, &linear_projection);
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

fn harness_source_intent(record: &DecisionContractRecord) -> HarnessSourceIntent {
	let contract = record.contract();

	HarnessSourceIntent {
		contract_id: contract.contract_id().to_owned(),
		status: record.status().as_str().to_owned(),
		summary: contract.source_intent().summary().to_owned(),
		source_issue_identifier: contract
			.source_intent()
			.source_issue_identifier()
			.map(str::to_owned),
	}
}

fn harness_outcome_contract(record: &DecisionContractRecord) -> HarnessOutcomeContract {
	let contract = record.contract();
	let readiness = contract.execution_readiness();
	let links = contract.links();

	HarnessOutcomeContract {
		contract_id: contract.contract_id().to_owned(),
		status: record.status().as_str().to_owned(),
		source_issue_id: record.source_issue_id().map(str::to_owned),
		ready_for_issue_shaping: readiness.ready_for_issue_shaping(),
		missing_decision_count: readiness.missing_decisions().len(),
		generated_issue_ids: links.generated_issue_ids().to_vec(),
		generated_issue_identifiers: links.generated_issue_identifiers().to_vec(),
		execution_program_node_ids: links.execution_program_node_ids().to_vec(),
		conflict_domains: readiness.conflict_domains().to_vec(),
	}
}

fn harness_outcome_program(record: &ExecutionProgramRecord) -> HarnessOutcomeProgram {
	let program = record.program();
	let nodes = program
		.nodes()
		.iter()
		.map(|node| {
			let linear_issue = node.linear_issue();

			HarnessOutcomeProgramNode {
				node_id: node.node_id().to_owned(),
				program_stage: node.stage().as_str().to_owned(),
				queue_intent: node.queue_intent().as_str().to_owned(),
				linear_issue_id: linear_issue.map(|issue| issue.issue_id().to_owned()),
				linear_issue_identifier: linear_issue
					.map(|issue| issue.issue_identifier().to_owned()),
				conflict_domains: node
					.conflict_domains()
					.iter()
					.map(harness_conflict_domain_label)
					.collect(),
			}
		})
		.collect::<Vec<_>>();

	HarnessOutcomeProgram {
		program_id: record.program_id().to_owned(),
		source_contract_id: record.source_contract_id().map(str::to_owned),
		node_count: nodes.len(),
		nodes,
	}
}

fn harness_conflict_domain_label(domain: &ExecutionConflictDomain) -> String {
	format!("{}:{}", domain.kind().as_str(), domain.key())
}

pub(super) fn harness_outcome_signals(
	events: &[PrivateExecutionEvent],
	_outcome: HarnessOutcomeKind,
	error_class: Option<&str>,
) -> HarnessOutcomeSignals {
	let mut signals = HarnessOutcomeSignals::default();

	if let Some(error_class) =
		error_class.filter(|class| harness_error_class_is_validation_failure(class))
	{
		signals.validation_failure_count += 1;

		signals.validation_failure_classes.insert(error_class.to_owned());
	}

	for event in events {
		match event.event_type() {
			"phase_goal_completed" | "phase_goal_set" =>
				push_phase_goal_signal(&mut signals, event),
			"review_checkpoint" => push_review_signal(&mut signals, event.payload()),
			"loop_guardrail_checkpoint" => push_guardrail_signal(&mut signals, event.payload()),
			"authority_boundary_check" =>
				push_authority_boundary_signal(&mut signals, event.payload()),
			"architecture_recovery_terminal" => {
				push_architecture_recovery_signal(&mut signals, event.payload());
			},
			"progress_checkpoint" => push_progress_signal(&mut signals, event.payload()),
			_ => {},
		}
	}

	signals
}

fn harness_error_class_is_validation_failure(error_class: &str) -> bool {
	error_class.starts_with("repo_gate_")
		|| matches!(error_class, "validation_repeat" | "validation_failure_repeated")
}

fn push_phase_goal_signal(signals: &mut HarnessOutcomeSignals, event: &PrivateExecutionEvent) {
	let payload = event.payload();
	let nested = payload.get("payload").unwrap_or(payload);
	let signal = json_string(nested.get("signal")).or_else(|| json_string(payload.get("signal")));
	let phase = json_string(nested.get("phase")).or_else(|| json_string(payload.get("phase")));
	let status = json_string(nested.get("status"));

	if signal.as_deref() == Some("validation_fail") {
		signals.validation_failure_count += 1;

		signals.validation_failure_classes.insert(String::from("phase_goal_validation_fail"));
	}
	if phase.as_deref().is_some_and(|phase| phase.contains("repair")) {
		signals.repair_phase_events += 1;
	}

	signals.phase_goals.push(HarnessPhaseGoalOutcome {
		event_type: event.event_type().to_owned(),
		phase,
		signal,
		status,
	});
}

fn push_review_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if let Some(status) = json_string(payload.get("status")) {
		signals.review_statuses.insert(status);
	}

	signals.nonclean_rounds = signals
		.nonclean_rounds
		.max(payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0));

	let review = payload.get("review").unwrap_or(payload);

	signals.accepted_finding_count += json_array_len(review.get("accepted_findings"));
	signals.rejected_finding_count += json_array_len(review.get("rejected_findings"));
}

fn push_guardrail_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if let Some(reason) = json_string(payload.get("reason")) {
		signals.guardrail_reasons.insert(reason);
	}
	if let Some(error_class) = json_string(payload.get("source_error_class"))
		&& harness_error_class_is_validation_failure(&error_class)
	{
		signals.validation_failure_count += 1;

		signals.validation_failure_classes.insert(error_class);
	}
}

fn push_authority_boundary_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if let Some(disposition) = json_string(payload.get("disposition")) {
		signals.authority_boundary_dispositions.insert(disposition.clone());

		if disposition != "within_authority" {
			signals.authority_boundary_failed_check_count += 1;
		}
		if matches!(disposition.as_str(), "requires_human" | "insufficient_evidence")
			&& json_array_len(payload.get("improvement_signals")) == 0
			&& authority_boundary_final_reason_mentions_underspecified(payload)
		{
			let target = first_decision_contract_target(payload)
				.unwrap_or_else(|| String::from("issue:local-readback"));

			signals.authority_boundary_candidates.push(HarnessImprovementCandidateSummary {
				kind: String::from("underspecified_decision_contract"),
				reason_code: String::from("authority_underspecified"),
				target,
				source_event_count: 1,
				recommendation: String::from(
					"Add explicit authority-envelope fields before retrying autonomous recovery.",
				),
			});
		}
	}
	if let Some(improvement_signals) = payload.get("improvement_signals").and_then(Value::as_array)
	{
		for signal in improvement_signals {
			let Some(kind) = json_string(signal.get("kind")) else {
				continue;
			};
			let Some(reason_code) = json_string(signal.get("reason_code")) else {
				continue;
			};
			let Some(target) = json_string(signal.get("target")) else {
				continue;
			};
			let Some(recommendation) = json_string(signal.get("recommendation")) else {
				continue;
			};

			signals.authority_boundary_candidates.push(HarnessImprovementCandidateSummary {
				kind,
				reason_code,
				target,
				source_event_count: 1,
				recommendation,
			});
		}
	}
}

fn push_architecture_recovery_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if json_string(payload.get("reason_code")).as_deref() == Some("architecture_recovery_exhausted")
	{
		signals.architecture_recovery_budget_exhausted_count += 1;
	}
}

fn push_progress_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if json_string(payload.get("phase")).is_some_and(|phase| phase.contains("repair")) {
		signals.repair_phase_events += 1;
	}
}

pub(super) fn harness_linear_projection(
	linear_records: &[LinearExecutionEventRecord],
) -> HarnessLinearProjectionSummary {
	let mut event_types =
		linear_records.iter().map(|record| record.event_type.clone()).collect::<Vec<_>>();

	event_types.sort();
	event_types.dedup();

	let final_record = linear_records
		.iter()
		.max_by(|left, right| left.event_timestamp.cmp(&right.event_timestamp));

	HarnessLinearProjectionSummary {
		event_types,
		final_event_type: final_record.map(|record| record.event_type.clone()),
		final_error_class: final_record.and_then(|record| record.error_class.clone()),
		final_terminal_path: final_record.and_then(|record| record.terminal_path.clone()),
	}
}

pub(super) fn harness_pr_urls(
	explicit_pr_url: Option<&str>,
	linear_records: &[LinearExecutionEventRecord],
) -> Vec<String> {
	let mut pr_urls =
		explicit_pr_url.into_iter().map(str::to_owned).collect::<std::collections::BTreeSet<_>>();

	for record in linear_records {
		if let Some(pr_url) = &record.pr_url {
			pr_urls.insert(pr_url.clone());
		}
	}

	pr_urls.into_iter().collect()
}
