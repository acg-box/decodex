use crate::{
	orchestrator::status::{
		self, HashSet, IssueTracker, LoopGuardrailReason, OperatorQueuedIssueStatus, ServiceConfig,
		StateStore, TrackerIssue, WorkflowDocument, compare_issue_candidates,
		queue::{
			classification,
			models::{QueuedCandidateStatusPlan, QueuedIssueStatusOutcome},
		},
	},
	prelude::Result,
	tracker,
};

pub(crate) fn build_queued_candidate_statuses<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<Vec<OperatorQueuedIssueStatus>>
where
	T: IssueTracker,
{
	Ok(build_queued_candidate_status_plan(tracker, project, workflow, state_store)?.statuses)
}

pub(crate) fn build_queued_candidate_status_plan<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<QueuedCandidateStatusPlan>
where
	T: IssueTracker,
{
	let queue_label = tracker::automation_queue_label(project.service_id());
	let retained_post_review_issue_ids = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.map(|mapping| mapping.issue_id().to_owned())
		.collect::<HashSet<_>>();
	let success_state = workflow.frontmatter().tracker().success_state();
	let mut issues = tracker.list_issues_with_label(&queue_label)?;

	issues.sort_by(compare_issue_candidates);

	let mut statuses = Vec::new();
	let mut guardrail_commands = Vec::new();

	for issue in issues {
		if status::is_terminal_issue(&issue, workflow)
			|| queued_issue_is_retained_post_review_lane(
				&issue,
				success_state,
				&retained_post_review_issue_ids,
			) {
			continue;
		}

		let outcome = operator_queued_issue_status_with_commands(
			tracker,
			project,
			workflow,
			state_store,
			issue,
		)?;

		if let Some(command) = outcome.guardrail_command {
			guardrail_commands.push(command);
		}

		statuses.push(outcome.status);
	}

	Ok(QueuedCandidateStatusPlan { statuses, guardrail_commands })
}

pub(crate) fn queued_issue_is_retained_post_review_lane(
	issue: &TrackerIssue,
	success_state: &str,
	retained_post_review_issue_ids: &HashSet<String>,
) -> bool {
	issue.state.name == success_state && retained_post_review_issue_ids.contains(&issue.id)
}

pub(crate) fn queued_issue_blocker_identifiers(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
	reason: &str,
) -> Vec<String> {
	if reason != "open_tracker_blockers"
		&& reason != LoopGuardrailReason::DependencyProgramStale.error_class()
	{
		return Vec::new();
	}

	issue
		.blockers
		.iter()
		.filter(|blocker| !status::state_name_is_terminal(&blocker.state.name, workflow))
		.map(|blocker| blocker.identifier.clone())
		.collect()
}

fn operator_queued_issue_status_with_commands<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue: TrackerIssue,
) -> Result<QueuedIssueStatusOutcome>
where
	T: IssueTracker,
{
	let (classification, reason, guardrail_command) =
		classification::classify_queued_issue_with_command(
			tracker,
			project,
			workflow,
			state_store,
			&issue,
		)?;
	let blocker_identifiers = queued_issue_blocker_identifiers(&issue, workflow, reason);
	let attention = status::operator_queued_issue_attention_status(
		tracker,
		project,
		workflow,
		state_store,
		&issue,
		reason,
	)?;

	Ok(QueuedIssueStatusOutcome {
		status: OperatorQueuedIssueStatus {
			project_id: project.service_id().to_owned(),
			issue_id: issue.id,
			issue_identifier: issue.identifier,
			title: issue.title,
			author: issue.author,
			state: issue.state.name,
			priority: issue.priority,
			created_at: issue.created_at,
			classification: classification.to_owned(),
			reason: reason.to_owned(),
			attention,
			blocker_identifiers,
		},
		guardrail_command,
	})
}
