use crate::{
	orchestrator::{
		CONTINUATION_PENDING_RUN_STATUS, dispatch_policy,
		dispatch_policy::{
			ErrorKind, IssueDispatchMode, IssueTracker, Path, PathBuf, Result, RetryIssueStateHint,
			ServiceConfig, StateStore, TERMINAL_GUARD_MARKER_FILE, TERMINAL_GUARDED_RUN_STATUS,
			TrackerIssue, WorkflowDocument, fs,
		},
	},
	state, tracker,
};

pub(crate) fn issue_passes_retry_dispatch_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	hint: RetryIssueStateHint<'_>,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	issue_passes_retry_retention_policy(tracker, issue, project, workflow, state_store, hint)
}

pub(crate) fn issue_passes_retry_retention_policy<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	hint: RetryIssueStateHint<'_>,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let continuation_startable_snapshot = hint
		.preferred_issue_state
		.is_some_and(|state| state == tracker_policy.in_progress_state())
		&& hint.preferred_initial_issue_state.is_some_and(|state| state == issue.state.name)
		&& tracker_policy.startable_states().iter().any(|candidate| candidate == &issue.state.name);

	if !issue_has_retry_or_continuation_ownership(tracker, issue, project, state_store)?
		|| (issue.state.name != tracker_policy.in_progress_state()
			&& !continuation_startable_snapshot)
		|| issue.has_label(tracker_policy.opt_out_label())
		|| issue.has_label(tracker_policy.needs_attention_label())
		|| issue_is_terminal_retry_guarded(issue, project, state_store)?
	{
		return Ok(false);
	}

	Ok(!dispatch_policy::ordinary_dispatch_blocked_by_retained_review_handoff(
		project.service_id(),
		issue,
		state_store,
	)?)
}

pub(crate) fn issue_has_service_ownership<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)
}

fn issue_has_retry_or_continuation_ownership<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	if issue_has_service_ownership(tracker, issue, project.service_id())? {
		return Ok(true);
	}

	let latest_allows_continuation_reentry =
		state_store.latest_run_attempt_for_issue(&issue.id)?.is_some_and(|attempt| {
			run_attempt_allows_continuation_reentry(project, state_store, &issue.id, &attempt)
				.unwrap_or(false)
		});

	if !latest_allows_continuation_reentry {
		return Ok(false);
	}

	tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_queue_label(project.service_id()),
	)
}

pub(crate) fn run_attempt_allows_continuation_reentry(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	attempt: &state::RunAttempt,
) -> Result<bool> {
	if attempt.status() == CONTINUATION_PENDING_RUN_STATUS {
		return Ok(true);
	}
	if attempt.status() != "interrupted" {
		return Ok(false);
	}

	let events = state_store.list_private_execution_events(
		project.service_id(),
		issue_id,
		attempt.run_id(),
		attempt.attempt_number(),
	)?;
	let has_progress_checkpoint =
		events.iter().any(|event| event.event_type() == "progress_checkpoint");
	let has_validation_fail = events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload().get("signal").and_then(|signal| signal.as_str())
				== Some("validation_fail")
	});
	let has_repair_goal = events.iter().any(|event| {
		event.event_type() == "phase_goal_set"
			&& event.payload().get("phase").and_then(|phase| phase.as_str())
				== Some("repair_validation_failures")
	});

	Ok(has_progress_checkpoint && has_validation_fail && has_repair_goal)
}

pub(crate) fn issue_is_terminal_retry_guarded(
	issue: &TrackerIssue,
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<bool> {
	Ok(state_store
		.latest_run_attempt_for_issue(&issue.id)?
		.is_some_and(|attempt| attempt.status() == TERMINAL_GUARDED_RUN_STATUS)
		|| terminal_guard_marker_path(project, &issue.identifier).exists())
}

pub(crate) fn write_terminal_guard_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<()> {
	let marker_path = worktree_path.join(TERMINAL_GUARD_MARKER_FILE);
	let marker_body = format!("run_id={run_id}\nattempt_number={attempt_number}\n");

	fs::write(marker_path, marker_body)?;

	Ok(())
}

pub(crate) fn write_retry_budget_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_budget_attempt_count: i64,
) -> Result<()> {
	state::write_run_retry_budget_attempt_count(
		worktree_path,
		run_id,
		attempt_number,
		retry_budget_attempt_count,
	)
}

pub(crate) fn retry_budget_base_for_issue_worktree(
	state_store: &StateStore,
	issue_id: &str,
	worktree_path: &Path,
) -> Result<i64> {
	Ok(state_store
		.retry_budget_attempt_count(issue_id)?
		.max(state::read_run_retry_budget_attempt_count(worktree_path)?.unwrap_or(0)))
}

pub(crate) fn retry_budget_base_for_dispatch_mode(
	state_store: &StateStore,
	issue_id: &str,
	worktree_path: &Path,
	dispatch_mode: IssueDispatchMode,
	preferred_retry_budget_base: Option<i64>,
) -> Result<i64> {
	let preferred_retry_budget_base = preferred_retry_budget_base.unwrap_or(0);

	if matches!(dispatch_mode, IssueDispatchMode::Normal | IssueDispatchMode::Program) {
		return Ok(preferred_retry_budget_base);
	}

	Ok(preferred_retry_budget_base.max(retry_budget_base_for_issue_worktree(
		state_store,
		issue_id,
		worktree_path,
	)?))
}

pub(crate) fn issue_retry_budget_exhausted(
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
) -> Result<bool> {
	if let Some(mapping) = state_store.worktree_for_issue(issue_id)? {
		return issue_retry_budget_exhausted_for_worktree(
			workflow,
			state_store,
			issue_id,
			mapping.worktree_path(),
		);
	}

	let retry_budget_attempts = state_store.retry_budget_attempt_count(issue_id)?;

	Ok(retry_budget_attempts >= i64::from(workflow.frontmatter().execution().max_attempts()))
}

pub(crate) fn issue_retry_budget_exhausted_for_worktree(
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	worktree_path: &Path,
) -> Result<bool> {
	let retry_budget_attempts =
		retry_budget_base_for_issue_worktree(state_store, issue_id, worktree_path)?;

	Ok(retry_budget_attempts >= i64::from(workflow.frontmatter().execution().max_attempts()))
}

pub(crate) fn clear_terminal_guard_marker(worktree_path: &Path) -> Result<()> {
	let marker_path = worktree_path.join(TERMINAL_GUARD_MARKER_FILE);

	match fs::remove_file(&marker_path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn terminal_guard_marker_path(project: &ServiceConfig, issue_identifier: &str) -> PathBuf {
	project.worktree_root().join(issue_identifier).join(TERMINAL_GUARD_MARKER_FILE)
}
