use records::LinearExecutionEventRecord;
use state::{DecisionContractRecord, ExecutionProgramRecord, PrivateExecutionEvent};

use crate::execution_program::ExecutionConflictDomain;

const HARNESS_OUTCOME_SCHEMA: &str = "decodex.harness_outcome/1";
const HARNESS_OUTCOME_EVENT_TYPE: &str = "harness_outcome";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HarnessOutcomeKind {
	ReviewHandoff,
	ReviewRepair,
	Closeout,
	RetryableFailure,
	TerminalFailure,
	ManualAttention,
}
impl HarnessOutcomeKind {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ReviewHandoff => "review_handoff",
			Self::ReviewRepair => "review_repair",
			Self::Closeout => "closeout",
			Self::RetryableFailure => "retryable_failure",
			Self::TerminalFailure => "terminal_failure",
			Self::ManualAttention => "manual_attention",
		}
	}

	fn validation_result(self, explicit: Option<&str>, signals: &HarnessOutcomeSignals) -> String {
		if let Some(result) = explicit {
			return result.to_owned();
		}

		if signals.validation_failure_count > 0 || matches!(self, Self::RetryableFailure) {
			return String::from("failed");
		}
		if matches!(self, Self::ReviewHandoff | Self::ReviewRepair | Self::Closeout) {
			return String::from("passed");
		}

		String::from("not_recorded")
	}
}

pub(crate) struct HarnessOutcomeRecordInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) outcome: HarnessOutcomeKind,
	pub(crate) error_class: Option<&'a str>,
	pub(crate) validation_result: Option<&'a str>,
	pub(crate) pr_url: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct HarnessImprovementCandidateSummary {
	pub(crate) kind: String,
	pub(crate) reason_code: String,
	pub(crate) target: String,
	pub(crate) source_event_count: usize,
	pub(crate) recommendation: String,
}

#[derive(Serialize)]
struct HarnessOutcomePayload {
	schema: &'static str,
	record_version: u16,
	source: HarnessOutcomeSource,
	contracts: Vec<HarnessOutcomeContract>,
	execution_programs: Vec<HarnessOutcomeProgram>,
	phase_goal_outcomes: Vec<HarnessPhaseGoalOutcome>,
	validation: HarnessValidationOutcome,
	repair: HarnessRepairOutcome,
	review: HarnessReviewOutcome,
	authority_boundary: HarnessAuthorityBoundaryOutcome,
	manual_attention: Option<HarnessManualAttentionOutcome>,
	pr_lifecycle: HarnessPrLifecycleOutcome,
	linear_projection: HarnessLinearProjectionSummary,
	improvement_candidates: Vec<HarnessImprovementCandidateSummary>,
}

#[derive(Serialize)]
struct HarnessOutcomeSource {
	project_id: String,
	issue_id: String,
	issue_identifier: String,
	run_id: String,
	attempt_number: i64,
	outcome: String,
	source_intents: Vec<HarnessSourceIntent>,
}

#[derive(Serialize)]
struct HarnessSourceIntent {
	contract_id: String,
	status: String,
	summary: String,
	source_issue_identifier: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct HarnessOutcomeContract {
	contract_id: String,
	status: String,
	source_issue_id: Option<String>,
	ready_for_issue_shaping: bool,
	missing_decision_count: usize,
	generated_issue_ids: Vec<String>,
	generated_issue_identifiers: Vec<String>,
	execution_program_node_ids: Vec<String>,
	conflict_domains: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct HarnessOutcomeProgram {
	program_id: String,
	source_contract_id: Option<String>,
	node_count: usize,
	nodes: Vec<HarnessOutcomeProgramNode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct HarnessOutcomeProgramNode {
	node_id: String,
	program_stage: String,
	queue_intent: String,
	linear_issue_id: Option<String>,
	linear_issue_identifier: Option<String>,
	conflict_domains: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct HarnessPhaseGoalOutcome {
	event_type: String,
	phase: Option<String>,
	signal: Option<String>,
	status: Option<String>,
}

#[derive(Serialize)]
struct HarnessValidationOutcome {
	result: String,
	failure_count: usize,
	failure_classes: Vec<String>,
}

#[derive(Serialize)]
struct HarnessRepairOutcome {
	attempt_number: i64,
	repair_attempt_observed: bool,
	repair_phase_events: usize,
}

#[derive(Serialize)]
struct HarnessReviewOutcome {
	statuses: Vec<String>,
	accepted_finding_count: usize,
	rejected_finding_count: usize,
	nonclean_rounds: i64,
}

#[derive(Serialize)]
struct HarnessAuthorityBoundaryOutcome {
	dispositions: Vec<String>,
	failed_check_count: usize,
	improvement_signal_count: usize,
}

#[derive(Serialize)]
struct HarnessManualAttentionOutcome {
	reason_code: String,
}

#[derive(Serialize)]
struct HarnessPrLifecycleOutcome {
	outcome: String,
	pr_urls: Vec<String>,
}

#[derive(Serialize)]
struct HarnessLinearProjectionSummary {
	event_types: Vec<String>,
	final_event_type: Option<String>,
	final_error_class: Option<String>,
	final_terminal_path: Option<String>,
}

#[derive(Default)]
struct HarnessOutcomeSignals {
	phase_goals: Vec<HarnessPhaseGoalOutcome>,
	validation_failure_count: usize,
	validation_failure_classes: std::collections::BTreeSet<String>,
	review_statuses: std::collections::BTreeSet<String>,
	accepted_finding_count: usize,
	rejected_finding_count: usize,
	nonclean_rounds: i64,
	repair_phase_events: usize,
	guardrail_reasons: std::collections::BTreeSet<String>,
	authority_boundary_dispositions: std::collections::BTreeSet<String>,
	authority_boundary_failed_check_count: usize,
	authority_boundary_candidates: Vec<HarnessImprovementCandidateSummary>,
	architecture_recovery_budget_exhausted_count: usize,
}

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
	let contracts = harness_contracts_for_issue(state_store, &input)?;
	let programs = harness_programs_for_contracts(state_store, input.project_id, &contracts)?;
	let linear_records = state_store.list_linear_execution_events(input.project_id, input.issue_id)?;
	let signals = harness_outcome_signals(&events, input.error_class);
	let payload = harness_outcome_payload(&input, &contracts, &programs, &linear_records, &signals)?;

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
		from_outcome.extend(harness_candidates_from_payload(event.payload()));
	}

	if !from_outcome.is_empty() {
		return from_outcome;
	}
	if events.is_empty() {
		return Vec::new();
	}

	let signals = harness_outcome_signals(events, None);

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
	let mut candidates = std::collections::BTreeMap::new();

	push_signal_candidates(&mut candidates, &input, &signals, &linear_projection);

	candidates.into_values().collect()
}

pub(super) fn record_harness_outcome_best_effort(
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

fn harness_contracts_for_issue(
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

fn harness_programs_for_contracts(
	state_store: &StateStore,
	project_id: &str,
	contracts: &[DecisionContractRecord],
) -> Result<Vec<ExecutionProgramRecord>> {
	let mut programs = Vec::new();
	let mut seen = std::collections::BTreeSet::new();

	for contract in contracts {
		for program in state_store.list_execution_programs_for_contract(
			project_id,
			contract.contract_id(),
		)? {
			let key = program.program_id().to_owned();

			if seen.insert(key) {
				programs.push(program);
			}
		}
	}

	programs.sort_by(|left, right| left.program_id().cmp(right.program_id()));

	Ok(programs)
}

fn harness_outcome_payload(
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
	let manual_attention =
		input.error_class.filter(|_| input.outcome == HarnessOutcomeKind::ManualAttention).map(
			|reason| HarnessManualAttentionOutcome { reason_code: reason.to_owned() },
		);
	let pr_lifecycle = HarnessPrLifecycleOutcome {
		outcome: input.outcome.as_str().to_owned(),
		pr_urls: harness_pr_urls(input.pr_url, linear_records),
	};
	let improvement_candidates = harness_improvement_candidates(
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

fn harness_outcome_signals(
	events: &[PrivateExecutionEvent],
	error_class: Option<&str>,
) -> HarnessOutcomeSignals {
	let mut signals = HarnessOutcomeSignals::default();

	if let Some(error_class) = error_class.filter(|class| class.starts_with("repo_gate_")) {
		signals.validation_failure_count += 1;

		signals.validation_failure_classes.insert(error_class.to_owned());
	}

	for event in events {
		match event.event_type() {
			"phase_goal_completed" | "phase_goal_set" => push_phase_goal_signal(&mut signals, event),
			"review_checkpoint" => push_review_signal(&mut signals, event.payload()),
			"loop_guardrail_checkpoint" => push_guardrail_signal(&mut signals, event.payload()),
			"authority_boundary_check" => push_authority_boundary_signal(&mut signals, event.payload()),
			"architecture_recovery_terminal" => {
				push_architecture_recovery_signal(&mut signals, event.payload());
			},
			"progress_checkpoint" => push_progress_signal(&mut signals, event.payload()),
			_ => {},
		}
	}

	signals
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

	signals.nonclean_rounds =
		signals.nonclean_rounds.max(payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0));

	let review = payload.get("review").unwrap_or(payload);

	signals.accepted_finding_count += json_array_len(review.get("accepted_findings"));
	signals.rejected_finding_count += json_array_len(review.get("rejected_findings"));
}

fn push_guardrail_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if let Some(reason) = json_string(payload.get("reason")) {
		signals.guardrail_reasons.insert(reason);
	}
	if let Some(error_class) = json_string(payload.get("source_error_class"))
		&& error_class.starts_with("repo_gate_")
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
	if let Some(improvement_signals) = payload
		.get("improvement_signals")
		.and_then(Value::as_array)
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
	if json_string(payload.get("reason_code")).as_deref()
		== Some("architecture_recovery_exhausted")
	{
		signals.architecture_recovery_budget_exhausted_count += 1;
	}
}

fn push_progress_signal(signals: &mut HarnessOutcomeSignals, payload: &Value) {
	if json_string(payload.get("phase")).is_some_and(|phase| phase.contains("repair")) {
		signals.repair_phase_events += 1;
	}
}

fn harness_linear_projection(
	linear_records: &[LinearExecutionEventRecord],
) -> HarnessLinearProjectionSummary {
	let mut event_types = linear_records
		.iter()
		.map(|record| record.event_type.clone())
		.collect::<Vec<_>>();

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

fn harness_pr_urls(
	explicit_pr_url: Option<&str>,
	linear_records: &[LinearExecutionEventRecord],
) -> Vec<String> {
	let mut pr_urls = explicit_pr_url
		.into_iter()
		.map(str::to_owned)
		.collect::<std::collections::BTreeSet<_>>();

	for record in linear_records {
		if let Some(pr_url) = &record.pr_url {
			pr_urls.insert(pr_url.clone());
		}
	}

	pr_urls.into_iter().collect()
}

fn harness_improvement_candidates(
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[HarnessOutcomeContract],
	programs: &[HarnessOutcomeProgram],
	signals: &HarnessOutcomeSignals,
	linear_projection: &HarnessLinearProjectionSummary,
) -> Vec<HarnessImprovementCandidateSummary> {
	let mut candidates = std::collections::BTreeMap::new();

	push_contract_candidates(&mut candidates, input, contracts, programs);
	push_signal_candidates(&mut candidates, input, signals, linear_projection);

	candidates.into_values().collect()
}

fn push_contract_candidates(
	candidates: &mut std::collections::BTreeMap<String, HarnessImprovementCandidateSummary>,
	input: &HarnessOutcomeRecordInput<'_>,
	contracts: &[HarnessOutcomeContract],
	programs: &[HarnessOutcomeProgram],
) {
	if contracts.is_empty() {
		insert_candidate(
			candidates,
			"missing_issue_template_field",
			"contract_provenance_missing",
			&format!("issue:{}", input.issue_identifier),
			0,
			"Add source intent and Decision Contract id/provenance to generated issue briefs.",
		);

		return;
	}

	for contract in contracts {
		if contract.missing_decision_count > 0 {
			insert_candidate(
				candidates,
				"underspecified_decision_contract",
				"missing_decisions",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Require missing decisions to be resolved before promotion or queueing.",
			);
		}
		if contract.generated_issue_ids.is_empty() && contract.generated_issue_identifiers.is_empty()
		{
			insert_candidate(
				candidates,
				"missing_issue_template_field",
				"generated_issue_links_missing",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Record generated issue ids or identifiers when research is promoted.",
			);
		}
		if contract.conflict_domains.is_empty() {
			insert_candidate(
				candidates,
				"missing_issue_template_field",
				"conflict_domains_missing",
				&format!("decision_contract:{}", contract.contract_id),
				0,
				"Require conflict-domain notes in contracts or generated issue templates.",
			);
		}
	}
	for program in programs.iter().filter(|program| program.node_count == 0) {
		insert_candidate(
			candidates,
			"stale_readiness_model",
			"execution_program_has_no_nodes",
			&format!("execution_program:{}", program.program_id),
			0,
			"Regenerate internal Execution Program readiness from the accepted contract.",
		);
	}
}

fn push_signal_candidates(
	candidates: &mut std::collections::BTreeMap<String, HarnessImprovementCandidateSummary>,
	input: &HarnessOutcomeRecordInput<'_>,
	signals: &HarnessOutcomeSignals,
	linear_projection: &HarnessLinearProjectionSummary,
) {
	if signals.validation_failure_count > 0 {
		insert_candidate(
			candidates,
			"weak_prompt",
			"validation_failed_after_generation",
			&format!("issue:{}", input.issue_identifier),
			signals.validation_failure_count,
			"Tighten phase prompts or preflight checks around the failing validation class.",
		);
	}
	if signals.accepted_finding_count > 0 {
		insert_candidate(
			candidates,
			"weak_prompt",
			"accepted_review_findings",
			&format!("issue:{}", input.issue_identifier),
			signals.accepted_finding_count,
			"Convert accepted reviewer findings into prompt, skill, or validator hardening.",
		);
	}

	for reason in &signals.guardrail_reasons {
		let (kind, recommendation) = guardrail_candidate_kind(reason);

		insert_candidate(
			candidates,
			kind,
			reason,
			&format!("issue:{}", input.issue_identifier),
			signals.guardrail_reasons.len(),
			recommendation,
		);
	}
	for candidate in &signals.authority_boundary_candidates {
		insert_candidate(
			candidates,
			&candidate.kind,
			&candidate.reason_code,
			&candidate.target,
			candidate.source_event_count,
			&candidate.recommendation,
		);
	}

	if signals.architecture_recovery_budget_exhausted_count > 0 {
		insert_candidate(
			candidates,
			"recovery_budget_exhausted",
			"architecture_recovery_exhausted",
			&format!("issue:{}", input.issue_identifier),
			signals.architecture_recovery_budget_exhausted_count,
			"Increase recovery evidence quality or require a new accepted architecture decision before retrying.",
		);
	}
	if input.error_class == Some("uncovered_direction")
		|| linear_projection.final_error_class.as_deref() == Some("uncovered_direction")
	{
		insert_candidate(
			candidates,
			"underspecified_decision_contract",
			"uncovered_direction",
			&format!("issue:{}", input.issue_identifier),
			1,
			"Feed the uncovered direction back into the Decision Contract before retrying.",
		);
	}
}

fn authority_boundary_final_reason_mentions_underspecified(payload: &Value) -> bool {
	let reason = payload
		.get("final_disposition")
		.and_then(|value| json_string(value.get("reason")))
		.or_else(|| json_string(payload.get("final_disposition_reason")));

	reason.is_some_and(|reason| {
		let reason = reason.to_ascii_lowercase();

		reason.contains("underspecified")
			|| reason.contains("missing contract")
			|| reason.contains("missing authority")
	})
}

fn first_decision_contract_target(payload: &Value) -> Option<String> {
	payload
		.get("decision_contract_ids")
		.and_then(Value::as_array)?
		.iter()
		.filter_map(Value::as_str)
		.find(|contract_id| !contract_id.is_empty())
		.map(|contract_id| format!("decision_contract:{contract_id}"))
}

fn guardrail_candidate_kind(reason: &str) -> (&'static str, &'static str) {
	match reason {
		"dependency_program_stale" => (
			"stale_readiness_model",
			"Refresh dependency readiness and queue-label policy for the affected program node.",
		),
		"uncovered_direction" => (
			"underspecified_decision_contract",
			"Feed the uncovered direction back into the Decision Contract before retrying.",
		),
		"validation_repeat" | "remaining_delta_unchanged" => (
			"missing_validator",
			"Promote the repeated failure into an earlier deterministic validator or fixture.",
		),
		_ => (
			"weak_prompt",
			"Tighten loop instructions so future attempts stop or repair earlier.",
		),
	}
}

fn insert_candidate(
	candidates: &mut std::collections::BTreeMap<String, HarnessImprovementCandidateSummary>,
	kind: &str,
	reason_code: &str,
	target: &str,
	source_event_count: usize,
	recommendation: &str,
) {
	let key = format!("{kind}:{reason_code}:{target}");

	candidates.entry(key).or_insert_with(|| HarnessImprovementCandidateSummary {
		kind: kind.to_owned(),
		reason_code: reason_code.to_owned(),
		target: target.to_owned(),
		source_event_count,
		recommendation: recommendation.to_owned(),
	});
}

fn harness_candidates_from_payload(payload: &Value) -> Vec<HarnessImprovementCandidateSummary> {
	payload
		.get("improvement_candidates")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|candidate| {
			Some(HarnessImprovementCandidateSummary {
				kind: json_string(candidate.get("kind"))?,
				reason_code: json_string(candidate.get("reason_code"))?,
				target: json_string(candidate.get("target"))?,
				source_event_count: candidate
					.get("source_event_count")
					.and_then(Value::as_u64)
					.and_then(|value| usize::try_from(value).ok())
					.unwrap_or(0),
				recommendation: json_string(candidate.get("recommendation"))?,
			})
		})
		.collect()
}

fn json_string(value: Option<&Value>) -> Option<String> {
	value.and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn json_array_len(value: Option<&Value>) -> usize {
	value.and_then(Value::as_array).map_or(0, Vec::len)
}
