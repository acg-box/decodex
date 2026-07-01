#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator::daemon_retry) enum RetryEntryRetentionDecision {
	Retain,
	Drop,
	Block,
}

pub(in crate::orchestrator::daemon_retry) enum ChildExitPhaseGoalRecovery {
	None,
	Continuation(PhaseGoalRecoveryContinuation),
	Terminalized,
}

pub(in crate::orchestrator::daemon_retry) struct ChildExitRetrySchedule<'a> {
	pub(in crate::orchestrator::daemon_retry) project_id: &'a str,
	pub(in crate::orchestrator::daemon_retry) issue_id: &'a str,
	pub(in crate::orchestrator::daemon_retry) run_id: &'a str,
	pub(in crate::orchestrator::daemon_retry) attempt_number: i64,
	pub(in crate::orchestrator::daemon_retry) continuation_initial_issue_state: Option<String>,
	pub(in crate::orchestrator::daemon_retry) dispatch_mode: IssueDispatchMode,
	pub(in crate::orchestrator::daemon_retry) kind: RetryKind,
	pub(in crate::orchestrator::daemon_retry) attempt: u32,
}

pub(in crate::orchestrator::daemon_retry) fn evaluate_post_review_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dispatch_mode: IssueDispatchMode,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker + ?Sized,
{
	match dispatch_mode {
		IssueDispatchMode::ReviewRepair => {
			Ok(if issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)? {
				RetryEntryRetentionDecision::Retain
			} else {
				RetryEntryRetentionDecision::Drop
			})
		},
		IssueDispatchMode::Closeout => Ok(match evaluate_closeout_dispatch_policy_with_inspector(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			&GhPullRequestReviewStateInspector {
				github_token_env_var: Some(project.github().token_env_var().to_owned()),
				github_command_path: project.github().command_path().map(Path::to_path_buf),
			},
		)? {
			CloseoutDispatchEligibility::Eligible => RetryEntryRetentionDecision::Retain,
			CloseoutDispatchEligibility::Ineligible => RetryEntryRetentionDecision::Drop,
			CloseoutDispatchEligibility::Blocked(_) => RetryEntryRetentionDecision::Block,
		}),
		_ => Ok(RetryEntryRetentionDecision::Drop),
	}
}

fn evaluate_retry_entry_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	entry: &RetryEntry,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker + ?Sized,
{
	if issue_has_blocking_lane_decision_evidence(project, state_store, &issue.id)? {
		return Ok(RetryEntryRetentionDecision::Drop);
	}

	if matches!(entry.dispatch_mode, IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout)
	{
		if entry.dispatch_mode == IssueDispatchMode::ReviewRepair
			&& issue_retry_budget_exhausted(workflow, state_store, &issue.id)?
		{
			return Ok(RetryEntryRetentionDecision::Drop);
		}

		return evaluate_post_review_retention_policy(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			entry.dispatch_mode,
		);
	}

	let preferred_issue_state = (entry.kind == RetryKind::Continuation)
		.then_some(workflow.frontmatter().tracker().in_progress_state());

	if issue_passes_retry_retention_policy(
		tracker,
		issue,
		project,
		workflow,
		state_store,
		RetryIssueStateHint {
			preferred_issue_state,
			preferred_initial_issue_state: entry.continuation_initial_issue_state.as_deref(),
		},
	)? {
		Ok(RetryEntryRetentionDecision::Retain)
	} else {
		Ok(RetryEntryRetentionDecision::Drop)
	}
}

pub(in crate::orchestrator) fn retry_entry_is_temporarily_blocked<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	entry: &RetryEntry,
) -> Result<bool>
where
	T: IssueTracker,
{
	let Some(issue) = refresh_issue(tracker, &entry.issue_id)? else {
		return Ok(false);
	};

	match evaluate_retry_entry_retention_policy(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
		entry,
	)? {
		RetryEntryRetentionDecision::Drop => return Ok(false),
		RetryEntryRetentionDecision::Block => return Ok(true),
		RetryEntryRetentionDecision::Retain => {},
	}

	if state_store.issue_has_active_shared_claim(project.service_id(), &entry.issue_id)? {
		return Ok(true);
	}

	Ok(false)
}

pub(in crate::orchestrator::daemon_retry) fn child_exit_retry_retention_decision<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	continuation_pending: bool,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker,
{
	if issue_has_blocking_lane_decision_evidence(context.project, context.state_store, &issue.id)? {
		return Ok(RetryEntryRetentionDecision::Drop);
	}

	if matches!(dispatch_mode, IssueDispatchMode::ReviewRepair | IssueDispatchMode::Closeout) {
		return evaluate_post_review_retention_policy(
			context.tracker,
			issue,
			context.project,
			context.workflow,
			context.state_store,
			dispatch_mode,
		);
	}

	let preferred_issue_state = continuation_pending
		.then_some(context.workflow.frontmatter().tracker().in_progress_state());

	if issue_passes_retry_retention_policy(
		context.tracker,
		issue,
		context.project,
		context.workflow,
		context.state_store,
		RetryIssueStateHint {
			preferred_issue_state,
			preferred_initial_issue_state: continuation_pending.then_some(initial_issue_state),
		},
	)? {
		Ok(RetryEntryRetentionDecision::Retain)
	} else {
		Ok(RetryEntryRetentionDecision::Drop)
	}
}
