use crate::{
	config::ServiceConfig,
	execution_program::ExecutionWorkflowPolicy,
	orchestrator::{self, OperatorExecutionProgramStatus, status_execution_programs},
	prelude::Result,
	state::{ExecutionProgramRecord, StateStore},
	tracker::IssueTracker,
	workflow::WorkflowDocument,
};

pub(crate) fn operator_execution_program_statuses_with_live_tracker<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> Result<Vec<OperatorExecutionProgramStatus>>
where
	T: IssueTracker + ?Sized,
{
	let policy = ExecutionWorkflowPolicy::from_workflow(project.service_id(), workflow)?;
	let mapped_issue_ids =
		status_execution_programs::operator_execution_program_mapped_issue_ids(records);
	let refreshed_issues = orchestrator::refresh_execution_program_issues(tracker, records)?;

	if mapped_issue_ids.iter().any(|issue_id| !refreshed_issues.contains_key(issue_id)) {
		crate::prelude::eyre::bail!("Execution Program tracker metadata was incomplete.");
	}

	let refreshed_programs = records
		.iter()
		.cloned()
		.map(|record| {
			orchestrator::refresh_execution_program_tracker_facts(
				tracker,
				state_store,
				project.service_id(),
				workflow,
				record,
				&refreshed_issues,
			)
		})
		.collect::<Result<Vec<_>>>()?;
	let context = orchestrator::execution_program_readiness_context(
		project.service_id(),
		workflow,
		state_store,
		&refreshed_programs,
	)?;
	let mut statuses = Vec::new();

	for refreshed in refreshed_programs {
		let record = &refreshed.record;
		let program = &refreshed.program;
		let evaluation = if let Some(source_contract_id) = record.source_contract_id() {
			let Some(contract) =
				state_store.decision_contract(project.service_id(), source_contract_id)?
			else {
				statuses.push(OperatorExecutionProgramStatus::missing_contract(record));

				continue;
			};

			program.evaluate(contract.contract(), &policy, &context)?
		} else {
			program.evaluate_issue_batch(&policy, &context)?
		};

		if program != record.program() {
			state_store.upsert_execution_program(project.service_id(), (*program).clone())?;
		}

		statuses.push(OperatorExecutionProgramStatus::from_summary(
			record,
			evaluation.operator_summary(),
			&evaluation,
		));
	}

	statuses.sort_by(|left, right| left.program_id.cmp(&right.program_id));

	Ok(statuses)
}
