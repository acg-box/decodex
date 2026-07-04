use crate::{
	orchestrator::{
		run_cycle,
		run_cycle::{
			IssueTracker, Result, ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
		},
	},
	tracker,
};

pub(in crate::orchestrator) fn queued_issues_for_dispatch<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
) -> Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	let queue_label = tracker::automation_queue_label(project.service_id());
	let issues = clear_terminal_queued_lane_labels(
		tracker,
		project,
		workflow,
		tracker.list_issues_with_label(&queue_label)?,
		dry_run,
	)?;

	if !dry_run {
		let plan =
			run_cycle::build_queued_candidate_status_plan(tracker, project, workflow, state_store)?;

		run_cycle::apply_queued_candidate_guardrail_commands(
			project,
			workflow,
			state_store,
			&plan.guardrail_commands,
		)?;
	}

	Ok(issues)
}

fn clear_terminal_queued_lane_labels<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issues: Vec<TrackerIssue>,
	dry_run: bool,
) -> Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	let mut nonterminal_issues = Vec::with_capacity(issues.len());

	for issue in issues {
		if run_cycle::is_terminal_issue(&issue, workflow) {
			if !dry_run {
				tracker::clear_automation_lane_labels(tracker, &issue, project.service_id())?;

				tracing::info!(
					project_id = project.service_id(),
					issue_id = issue.id,
					issue = issue.identifier,
					"Cleared automation lane labels from terminal queued issue."
				);
			}

			continue;
		}

		nonterminal_issues.push(issue);
	}

	Ok(nonterminal_issues)
}
