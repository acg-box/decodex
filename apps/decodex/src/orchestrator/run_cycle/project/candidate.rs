use crate::orchestrator::run_cycle::{
	self, IssueDispatchMode, IssueTracker, RecoveredRuntimeState, Result,
	SelectedIssueRunCandidate, ServiceConfig, StateStore, WorkflowDocument, project::queue,
};

pub(in crate::orchestrator) fn select_project_issue_run_candidate<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	recovered_state: RecoveredRuntimeState,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
{
	let selected_retry_issue = select_recovered_retry_issue_candidate(
		project,
		state_store,
		recovered_state,
		excluded_issue_ids,
	)?;
	let selected_post_review_issue = run_cycle::select_post_review_issue_candidate(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
	)?;

	if let Some(candidate) = selected_retry_issue.or(selected_post_review_issue) {
		return Ok(Some(candidate));
	}
	if let Some(candidate) = run_cycle::select_execution_program_run_candidate(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
	)? {
		return Ok(Some(candidate));
	}

	let issues =
		queue::queued_issues_for_dispatch(tracker, project, workflow, state_store, dry_run)?;

	Ok(run_cycle::select_issue_candidate_with_exclusions(
		tracker,
		issues,
		workflow,
		state_store,
		project.service_id(),
		excluded_issue_ids,
	)?
	.map(|issue| SelectedIssueRunCandidate::new(issue, IssueDispatchMode::Normal)))
}

fn select_recovered_retry_issue_candidate(
	project: &ServiceConfig,
	state_store: &StateStore,
	recovered_state: RecoveredRuntimeState,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>> {
	for issue in recovered_state.recoverable_issues {
		if excluded_issue_ids.contains(&issue.id.as_str()) {
			continue;
		}
		if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
			continue;
		}

		return Ok(Some(SelectedIssueRunCandidate::new(issue, IssueDispatchMode::Retry)));
	}

	Ok(None)
}
