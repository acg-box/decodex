use crate::{
	execution_program::ExecutionWorkflowPolicy,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, IssueTracker, PROGRAM_DISPATCH_SELECTED_EVENT_TYPE,
		PROGRAM_DISPATCH_SELECTED_SCHEMA, ProgramDispatchSelection, ProgramSchedulerSelection,
		ProgramSchedulerSummary, Result, RunSummary, SelectedIssueRunCandidate, ServiceConfig,
		StateStore, WorkflowDocument, eyre, program_reconciler,
	},
	state::PrivateExecutionEvent,
};

struct ProgramDispatchEventFields<'a> {
	project_id: &'a str,
	issue_id: &'a str,
	issue_identifier: &'a str,
	run_id: &'a str,
	attempt_number: i64,
	dispatch_mode: IssueDispatchMode,
}

pub(crate) fn record_program_dispatch_selected(
	state_store: &StateStore,
	project_id: &str,
	issue_run: &IssueRunPlan,
	program_dispatch: &ProgramDispatchSelection,
) -> Result<PrivateExecutionEvent> {
	record_program_dispatch_selected_fields(
		state_store,
		ProgramDispatchEventFields {
			project_id,
			issue_id: &issue_run.issue.id,
			issue_identifier: &issue_run.issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
			dispatch_mode: issue_run.dispatch_mode,
		},
		program_dispatch,
	)
}

pub(crate) fn record_program_dispatch_selected_for_summary(
	state_store: &StateStore,
	summary: &RunSummary,
	program_dispatch: &ProgramDispatchSelection,
) -> Result<PrivateExecutionEvent> {
	record_program_dispatch_selected_fields(
		state_store,
		ProgramDispatchEventFields {
			project_id: &summary.project_id,
			issue_id: &summary.issue_id,
			issue_identifier: &summary.issue_identifier,
			run_id: &summary.run_id,
			attempt_number: summary.attempt_number,
			dispatch_mode: summary.dispatch_mode,
		},
		program_dispatch,
	)
}

pub(crate) fn select_execution_program_run_candidate<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker + ?Sized,
{
	let ProgramSchedulerSelection { selected, summary } =
		select_execution_program_run_candidate_with_summary(
			tracker,
			project,
			workflow,
			state_store,
			excluded_issue_ids,
		)?;

	if summary.dispatchable_nodes > 0 || summary.programs_updated > 0 {
		tracing::info!(
			project_id = project.service_id(),
			programs_evaluated = summary.programs_evaluated,
			programs_updated = summary.programs_updated,
			dispatchable_nodes = summary.dispatchable_nodes,
			"Evaluated Execution Programs for direct graph dispatch."
		);
	}

	Ok(selected)
}

pub(crate) fn select_execution_program_run_candidate_with_summary<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
) -> Result<ProgramSchedulerSelection>
where
	T: IssueTracker + ?Sized,
{
	let records = state_store.list_execution_programs(project.service_id())?;

	if records.is_empty() {
		return Ok(ProgramSchedulerSelection {
			selected: None,
			summary: ProgramSchedulerSummary::default(),
		});
	}

	let policy = ExecutionWorkflowPolicy::from_workflow(project.service_id(), workflow)?;
	let refreshed_issues = program_reconciler::refresh_execution_program_issues(tracker, &records)?;
	let refreshed_programs = records
		.into_iter()
		.map(|record| {
			program_reconciler::refresh_execution_program_tracker_facts(
				tracker,
				state_store,
				project.service_id(),
				workflow,
				record,
				&refreshed_issues,
			)
		})
		.collect::<Result<Vec<_>>>()?;
	let context = program_reconciler::execution_program_readiness_context(
		project.service_id(),
		workflow,
		state_store,
		&refreshed_programs,
	)?;
	let mut summary = ProgramSchedulerSummary::default();
	let mut candidates = Vec::new();

	for refreshed in refreshed_programs {
		let evaluation = if let Some(source_contract_id) = refreshed.record.source_contract_id() {
			let Some(contract) =
				state_store.decision_contract(project.service_id(), source_contract_id)?
			else {
				continue;
			};

			refreshed.program.evaluate(contract.contract(), &policy, &context)?
		} else {
			refreshed.program.evaluate_issue_batch(&policy, &context)?
		};

		summary.programs_evaluated += 1;

		for node in evaluation.nodes() {
			if !node.dispatchable() {
				continue;
			}

			let Some(mapping) = node.linear_issue() else {
				continue;
			};

			if excluded_issue_ids.contains(&mapping.issue_id()) {
				continue;
			}

			let Some(snapshot) = refreshed.issues_by_node.get(node.node_id()) else {
				continue;
			};
			let Some(program_node) = refreshed
				.program
				.nodes()
				.iter()
				.find(|program_node| program_node.node_id() == node.node_id())
			else {
				continue;
			};

			summary.dispatchable_nodes += 1;

			candidates.push(
				SelectedIssueRunCandidate::new(snapshot.issue.clone(), IssueDispatchMode::Program)
					.with_program_dispatch(ProgramDispatchSelection {
						program_id: refreshed.record.program_id().to_owned(),
						node_id: node.node_id().to_owned(),
						source_contract_id: refreshed
							.record
							.source_contract_id()
							.map(str::to_owned),
						queue_intent: program_node.queue_intent().as_str().to_owned(),
					}),
			);
		}

		if refreshed.program != *refreshed.record.program() {
			state_store.upsert_execution_program(project.service_id(), refreshed.program)?;

			summary.programs_updated += 1;
		}
	}

	candidates
		.sort_by(|left, right| orchestrator::compare_issue_candidates(&left.issue, &right.issue));

	Ok(ProgramSchedulerSelection { selected: candidates.into_iter().next(), summary })
}

fn record_program_dispatch_selected_fields(
	state_store: &StateStore,
	fields: ProgramDispatchEventFields<'_>,
	program_dispatch: &ProgramDispatchSelection,
) -> Result<PrivateExecutionEvent> {
	let mut conflicting_event_id = None;

	for existing in state_store.list_private_execution_events(
		fields.project_id,
		fields.issue_id,
		fields.run_id,
		fields.attempt_number,
	)? {
		if existing.event_type() != PROGRAM_DISPATCH_SELECTED_EVENT_TYPE {
			continue;
		}
		if program_dispatch_event_matches(&existing, &fields, program_dispatch) {
			return Ok(existing);
		}

		conflicting_event_id = Some(existing.record_id().to_owned());

		break;
	}

	if let Some(record_id) = conflicting_event_id {
		eyre::bail!(
			"Conflicting Program dispatch selection event `{record_id}` already exists for `{}` attempt {}.",
			fields.run_id,
			fields.attempt_number
		);
	}

	state_store.append_private_execution_event(
		fields.project_id,
		fields.issue_id,
		fields.run_id,
		fields.attempt_number,
		PROGRAM_DISPATCH_SELECTED_EVENT_TYPE,
		serde_json::json!({
			"schema": PROGRAM_DISPATCH_SELECTED_SCHEMA,
			"record_version": 1,
			"issue": {
				"id": fields.issue_id,
				"identifier": fields.issue_identifier,
			},
			"run": {
				"run_id": fields.run_id,
				"attempt_number": fields.attempt_number,
				"dispatch_mode": fields.dispatch_mode.as_str(),
			},
			"execution_program": {
				"program_id": &program_dispatch.program_id,
				"node_id": &program_dispatch.node_id,
				"source_contract_id": &program_dispatch.source_contract_id,
				"queue_intent": &program_dispatch.queue_intent,
			},
		}),
	)
}

fn program_dispatch_event_matches(
	event: &PrivateExecutionEvent,
	fields: &ProgramDispatchEventFields<'_>,
	program_dispatch: &ProgramDispatchSelection,
) -> bool {
	let payload = event.payload();
	let source_contract_id =
		payload["execution_program"]["source_contract_id"].as_str().map(str::to_owned);

	payload["schema"] == PROGRAM_DISPATCH_SELECTED_SCHEMA
		&& payload["issue"]["id"] == fields.issue_id
		&& payload["issue"]["identifier"] == fields.issue_identifier
		&& payload["run"]["run_id"] == fields.run_id
		&& payload["run"]["attempt_number"] == fields.attempt_number
		&& payload["run"]["dispatch_mode"] == fields.dispatch_mode.as_str()
		&& payload["execution_program"]["program_id"] == program_dispatch.program_id
		&& payload["execution_program"]["node_id"] == program_dispatch.node_id
		&& source_contract_id == program_dispatch.source_contract_id
		&& payload["execution_program"]["queue_intent"] == program_dispatch.queue_intent
}
