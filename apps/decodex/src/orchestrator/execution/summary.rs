use crate::{
	agent::AppServerRunResult,
	config::ServiceConfig,
	orchestrator::{IssueDispatchMode, IssueRunPlan, RunSummary},
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

pub(crate) fn run_summary_from_issue_run(project_id: &str, issue_run: &IssueRunPlan) -> RunSummary {
	RunSummary {
		project_id: project_id.to_owned(),
		issue_id: issue_run.issue.id.clone(),
		issue_identifier: issue_run.issue.identifier.clone(),
		issue_state: issue_run.issue_state.clone(),
		initial_issue_state: issue_run.initial_issue_state.clone(),
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode: issue_run.dispatch_mode,
		branch_name: issue_run.worktree.branch_name.clone(),
		worktree_path: issue_run.worktree.path.clone(),
		attempt_number: issue_run.attempt_number,
		run_id: issue_run.run_id.clone(),
		continuation_pending: false,
	}
}

pub(super) fn continuation_boundary_summary(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue_run: &IssueRunPlan,
	run_result: &AppServerRunResult,
) -> RunSummary {
	tracing::info!(
		project_id = project.service_id(),
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		thread_id = run_result.thread_id,
		turn_count = run_result.turn_count,
		max_turns = workflow.frontmatter().execution().max_turns(),
		"Run reached a clean continuation boundary and will rely on the next bounded re-entry."
	);

	RunSummary {
		continuation_pending: true,
		..run_summary_from_issue_run(project.service_id(), issue_run)
	}
}

pub(crate) fn planned_issue_state_for_dispatch(
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	dispatch_mode: IssueDispatchMode,
	preferred_issue_state: Option<&str>,
) -> String {
	match dispatch_mode {
		IssueDispatchMode::Normal | IssueDispatchMode::Program =>
			workflow.frontmatter().tracker().in_progress_state().to_owned(),
		IssueDispatchMode::Retry => preferred_issue_state
			.filter(|state| {
				*state == workflow.frontmatter().tracker().in_progress_state()
					&& workflow
						.frontmatter()
						.tracker()
						.startable_states()
						.iter()
						.any(|candidate| candidate == &issue.state.name)
			})
			.map(|_| {
				preferred_issue_state
					.expect("filtered preferred issue state should exist")
					.to_owned()
			})
			.unwrap_or_else(|| issue.state.name.clone()),
		IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout => issue.state.name.clone(),
	}
}
