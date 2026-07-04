use crate::{
	config::ServiceConfig,
	execution_program::ExecutionWorkflowPolicy,
	orchestrator::{self, OperatorExecutionProgramStatus, status_execution_programs},
	prelude::Result,
	state::{ExecutionProgramRecord, StateStore},
	workflow::WorkflowDocument,
};

pub(crate) fn operator_execution_program_statuses_from_persisted(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	records: &[ExecutionProgramRecord],
) -> Result<Vec<OperatorExecutionProgramStatus>> {
	let policy = ExecutionWorkflowPolicy::from_workflow(project.service_id(), workflow)?;
	let context = status_execution_programs::operator_execution_program_readiness_context(
		project.service_id(),
		workflow,
		state_store,
		records,
	)?;
	let mut statuses = Vec::new();

	for record in records {
		let mut nodes = Vec::with_capacity(record.program().nodes().len());

		for node in record.program().nodes() {
			nodes.push(orchestrator::refresh_execution_program_local_lifecycle_facts(
				state_store,
				project.service_id(),
				node,
			)?);
		}

		let program = record.program().clone().with_nodes(nodes)?;
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

		statuses.push(OperatorExecutionProgramStatus::from_summary(
			record,
			evaluation.operator_summary(),
			&evaluation,
		));
	}

	statuses.sort_by(|left, right| left.program_id.cmp(&right.program_id));

	Ok(statuses)
}
