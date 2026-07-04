//! Execution Program readback for operator status snapshots.

mod context;
mod live;
mod persisted;

pub(crate) use self::context::{
	operator_execution_program_mapped_issue_ids, operator_execution_program_readiness_context,
};

use crate::{
	config::ServiceConfig, orchestrator::OperatorExecutionProgramReadback, prelude::Result,
	state::StateStore, tracker::IssueTracker, workflow::WorkflowDocument,
};

pub(super) fn operator_execution_program_statuses<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<OperatorExecutionProgramReadback>
where
	T: IssueTracker + ?Sized,
{
	let records = state_store.list_execution_programs(project.service_id())?;

	if records.is_empty() {
		return Ok(OperatorExecutionProgramReadback {
			statuses: Vec::new(),
			issue_metadata_unavailable: false,
		});
	}

	match live::operator_execution_program_statuses_with_live_tracker(
		tracker,
		project,
		workflow,
		state_store,
		&records,
	) {
		Ok(statuses) =>
			Ok(OperatorExecutionProgramReadback { statuses, issue_metadata_unavailable: false }),
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project.service_id(),
				"Skipped live tracker metadata hydration for Execution Program status; sensitive tracker details were withheld."
			);

			Ok(OperatorExecutionProgramReadback {
				statuses: persisted::operator_execution_program_statuses_from_persisted(
					project,
					workflow,
					state_store,
					&records,
				)?,
				issue_metadata_unavailable: true,
			})
		},
	}
}
