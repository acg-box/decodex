mod context;
mod diagnostic;
mod error;
mod record;

pub(crate) use self::{
	diagnostic::truncate_private_diagnostic_text, error::AppServerZeroEvidenceStartFailure,
};

use crate::orchestrator::execution_failure::{
	self, IssueRunPlan, Report, ServiceConfig, StateStore,
};

pub(crate) fn promote_zero_evidence_app_server_start_failure(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	error: Report,
) -> Report {
	let writeback_disposition = execution_failure::run_failure_writeback_disposition(&error);

	if writeback_disposition.requires_terminal_attention()
		|| writeback_disposition.preserves_retry_through_zero_evidence()
	{
		return error;
	}

	match context::zero_evidence_app_server_start_failure_context(project, state_store, issue_run) {
		Ok(Some(context)) => {
			let diagnostic = diagnostic::zero_evidence_app_server_start_failure_diagnostic(&error);

			if let Err(record_error) = record::record_zero_evidence_app_server_start_failure(
				project,
				state_store,
				issue_run,
				&context,
				&diagnostic,
			) {
				tracing::warn!(
					?record_error,
					project_id = project.service_id(),
					issue_id = issue_run.issue.id,
					issue = issue_run.issue.identifier,
					run_id = issue_run.run_id,
					attempt = issue_run.attempt_number,
					"Failed to record zero-evidence app-server start failure evidence."
				);
			}

			Report::new(AppServerZeroEvidenceStartFailure::new(
				issue_run.issue.identifier.clone(),
				issue_run.run_id.clone(),
			))
			.wrap_err(error)
		},
		Ok(None) => error,
		Err(context_error) => {
			tracing::warn!(
				?context_error,
				project_id = project.service_id(),
				issue_id = issue_run.issue.id,
				issue = issue_run.issue.identifier,
				run_id = issue_run.run_id,
				attempt = issue_run.attempt_number,
				"Failed to classify app-server start failure evidence."
			);

			error
		},
	}
}
