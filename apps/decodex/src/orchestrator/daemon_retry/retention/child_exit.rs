use crate::orchestrator::daemon_retry::{
	self, ChildExitRetryContext, IssueTracker, Result, RetryEntryLifecycle, RetryIssueStateHint,
	TrackerIssue,
	retention::{RetryEntryRetentionDecision, post_review},
};

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
		return post_review::evaluate_post_review_retention_policy(
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
