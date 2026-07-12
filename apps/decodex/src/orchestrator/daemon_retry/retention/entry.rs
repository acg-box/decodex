use crate::orchestrator::daemon_retry::{
	self, IssueTracker, Result, RetryEntry, RetryEntryLifecycle, RetryIssueStateHint, RetryKind,
	ServiceConfig, StateStore, TrackerIssue, WorkflowDocument,
	retention::{RetryEntryRetentionDecision, post_review},
};

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
			&& daemon_retry::issue_retry_budget_exhausted(
				workflow,
				state_store,
				project.service_id(),
				&issue.id,
			)? {
			return Ok(RetryEntryRetentionDecision::Drop);
		}

		return post_review::evaluate_post_review_retention_policy(
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
