use crate::orchestrator::daemon_retry::{
	self, ChildExitRetryContext, CloseoutDispatchEligibility, GhPullRequestReviewStateInspector,
	IssueDispatchMode, IssueTracker, Path, PhaseGoalRecoveryContinuation, Result, RetryEntry,
	RetryEntryLifecycle, RetryIssueStateHint, RetryKind, ServiceConfig, StateStore, TrackerIssue,
	WorkflowDocument,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RetryEntryRetentionDecision {
	Retain,
	Drop,
	Block,
}

pub(crate) enum ChildExitPhaseGoalRecovery {
	None,
	Continuation(PhaseGoalRecoveryContinuation),
	Terminalized,
}

pub(crate) struct ChildExitRetrySchedule<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) continuation_initial_issue_state: Option<String>,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) kind: RetryKind,
	pub(crate) attempt: u32,
}

pub(crate) fn retry_entry_is_temporarily_blocked<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	entry: &RetryEntry,
) -> Result<bool>
where
	T: IssueTracker,
{
	let Some(issue) = daemon_retry::refresh_issue(tracker, &entry.issue_id)? else {
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

pub(crate) fn child_exit_retry_retention_decision<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	initial_issue_state: &str,
	lifecycle: RetryEntryLifecycle,
	continuation_pending: bool,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker,
{
	if daemon_retry::issue_has_blocking_lane_decision_evidence(
		context.project,
		context.state_store,
		&issue.id,
	)? {
		return Ok(RetryEntryRetentionDecision::Drop);
	}
	if lifecycle != RetryEntryLifecycle::Active {
		return evaluate_post_review_retention_policy(
			context.tracker,
			issue,
			context.project,
			context.workflow,
			context.state_store,
			lifecycle,
		);
	}

	let preferred_issue_state = continuation_pending
		.then_some(context.workflow.frontmatter().tracker().in_progress_state());

	if daemon_retry::issue_passes_retry_retention_policy(
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

fn evaluate_post_review_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lifecycle: RetryEntryLifecycle,
) -> Result<RetryEntryRetentionDecision>
where
	T: IssueTracker + ?Sized,
{
	match lifecycle {
		RetryEntryLifecycle::ReviewRepair => Ok(
			if daemon_retry::issue_passes_review_repair_dispatch_policy(
				tracker, issue, project, workflow,
			)? {
				RetryEntryRetentionDecision::Retain
			} else {
				RetryEntryRetentionDecision::Drop
			},
		),
		RetryEntryLifecycle::Closeout => Ok(
			match daemon_retry::evaluate_closeout_dispatch_policy_with_inspector(
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
			},
		),
		RetryEntryLifecycle::Active => Ok(RetryEntryRetentionDecision::Drop),
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
	if daemon_retry::issue_has_blocking_lane_decision_evidence(project, state_store, &issue.id)? {
		return Ok(RetryEntryRetentionDecision::Drop);
	}
	if entry.lifecycle != RetryEntryLifecycle::Active {
		if entry.lifecycle == RetryEntryLifecycle::ReviewRepair
			&& daemon_retry::issue_retry_budget_exhausted(workflow, state_store, &issue.id)?
		{
			return Ok(RetryEntryRetentionDecision::Drop);
		}

		return evaluate_post_review_retention_policy(
			tracker,
			issue,
			project,
			workflow,
			state_store,
			entry.lifecycle,
		);
	}

	let preferred_issue_state = (entry.kind == RetryKind::Continuation)
		.then_some(workflow.frontmatter().tracker().in_progress_state());

	if daemon_retry::issue_passes_retry_retention_policy(
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
