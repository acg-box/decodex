use std::{
	path::Path,
	process::ExitStatus,
	time::{Duration, Instant},
};

use color_eyre::Report;
use serde_json::json;
use time::OffsetDateTime;

use crate::{state, tracker::TrackerIssue};

use super::{
	ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_RETRY_KIND,
	CONTINUATION_PENDING_RUN_STATUS, CONTINUATION_RETRY_DELAY_MS, ChildExitRetryContext,
	ChildRunRef, CloseoutDispatchEligibility, FAILURE_RETRY_BASE_DELAY_MS,
	GhPullRequestReviewStateInspector, IssueDispatchMode, IssueRunPlan, IssueTracker,
	LaneDecisionSnapshot, PhaseGoalRecoveryContinuation, Result, RetainedPartialProgress,
	RetryEntry, RetryIssueStateHint, RetryKind, RetryQueue, ServiceConfig, StateStore,
	TERMINAL_GUARDED_RUN_STATUS, TerminalFailureWritebackRuntime, WorkflowDocument,
	WorktreeManager, WorktreeSpec, apply_terminal_failure_writeback, clear_worktree_retry_schedule,
	configured_public_projection_privacy_classifier, decide_lane_next_action,
	evaluate_closeout_dispatch_policy_with_inspector, handle_failure,
	issue_has_blocking_lane_decision_evidence, issue_passes_retry_retention_policy,
	issue_passes_review_repair_dispatch_policy, issue_retry_budget_exhausted,
	lane_decision_blocks_automatic_execution, mark_run_attempt_if_active,
	recover_phase_goal_continuation, refresh_issue, relative_worktree_path,
	resolve_child_exit_run_attempt, run_failure_requires_terminal_attention,
	superseded_run_disposition, worktree_has_tracked_changes, write_retry_budget_marker,
	write_terminal_guard_marker,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RetryEntryRetentionDecision {
	Retain,
	Drop,
	Block,
}

enum ChildExitPhaseGoalRecovery {
	None,
	Continuation(PhaseGoalRecoveryContinuation),
	Terminalized,
}

struct ChildExitRetrySchedule<'a> {
	project_id: &'a str,
	issue_id: &'a str,
	run_id: &'a str,
	attempt_number: i64,
	continuation_initial_issue_state: Option<String>,
	dispatch_mode: IssueDispatchMode,
	kind: RetryKind,
	attempt: u32,
}

fn evaluate_post_review_retention_policy<T>(
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
		IssueDispatchMode::ReviewRepair =>
			Ok(if issue_passes_review_repair_dispatch_policy(tracker, issue, project, workflow)? {
				RetryEntryRetentionDecision::Retain
			} else {
				RetryEntryRetentionDecision::Drop
			}),
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

pub(super) fn retry_entry_is_temporarily_blocked<T>(
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

pub(super) fn schedule_retry_after_child_exit<T>(
	mut context: ChildExitRetryContext<'_, T>,
	child: ChildRunRef<'_>,
	#[cfg(test)] _retry_project_slug: &str,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	exit_status: ExitStatus,
) -> Result<()>
where
	T: IssueTracker,
{
	let Some(run_attempt) = resolve_child_exit_run_attempt(context.state_store, child)? else {
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			"Daemon child exited without a matching recorded run attempt; skipping retry scheduling."
		);

		return Ok(());
	};

	if !exit_status.success() {
		mark_run_attempt_if_active(context.state_store, run_attempt.run_id(), "failed")?;
	}

	let Some(run_attempt) = context.state_store.run_attempt(run_attempt.run_id())? else {
		return Ok(());
	};

	if superseded_run_disposition(context.state_store, &run_attempt)?.is_some() {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, child.issue_id)?;

		return Ok(());
	}

	let issue_id = run_attempt.issue_id();
	let Some(issue) = refresh_issue(context.tracker, issue_id)? else {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	};
	let continuation_pending =
		exit_status.success() && run_attempt.status() == CONTINUATION_PENDING_RUN_STATUS;

	if !exit_status.success() && run_attempt.status() != "failed" {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	}

	let retention_decision = child_exit_retry_retention_decision(
		&context,
		&issue,
		initial_issue_state,
		dispatch_mode,
		continuation_pending,
	)?;

	if retention_decision == RetryEntryRetentionDecision::Drop {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	}

	let recovered_phase_goal_continuation = match recover_child_exit_phase_goal(
		&mut context,
		&issue,
		child,
		issue_id,
		initial_issue_state,
		dispatch_mode,
		exit_status.success(),
	)? {
		ChildExitPhaseGoalRecovery::None => None,
		ChildExitPhaseGoalRecovery::Continuation(recovery) => Some(recovery),
		ChildExitPhaseGoalRecovery::Terminalized => return Ok(()),
	};
	let (kind, attempt, continuation_initial_issue_state) = if continuation_pending {
		(
			RetryKind::Continuation,
			u32::try_from(run_attempt.attempt_number()).unwrap_or(u32::MAX).max(1),
			Some(initial_issue_state.to_owned()),
		)
	} else if recovered_phase_goal_continuation.is_some() {
		context
			.state_store
			.update_run_status(run_attempt.run_id(), CONTINUATION_PENDING_RUN_STATUS)?;

		(
			RetryKind::Continuation,
			u32::try_from(run_attempt.attempt_number()).unwrap_or(u32::MAX).max(1),
			Some(initial_issue_state.to_owned()),
		)
	} else if exit_status.success() {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	} else {
		let retry_budget_attempts = child_exit_retry_budget_attempt_count(&context, &issue, child)?;
		let retry_budget_limit = child_exit_retry_budget_limit(&context, &issue, child)?;

		if retry_budget_attempts >= retry_budget_limit {
			return terminalize_exhausted_child_exit_retry(
				context,
				issue,
				child,
				initial_issue_state,
				dispatch_mode,
				retry_budget_attempts,
			);
		}

		(RetryKind::Failure, retry_budget_attempts, None)
	};
	let lane_snapshot = LaneDecisionSnapshot::child_exit_retry(
		issue.identifier.clone(),
		run_attempt.run_id().to_owned(),
		run_attempt.attempt_number(),
		dispatch_mode,
		kind == RetryKind::Continuation,
		Some(kind),
		0,
		false,
		false,
	);
	let lane_decision = decide_lane_next_action(&lane_snapshot);

	context.state_store.append_private_execution_event(
		context.project.service_id(),
		issue_id,
		run_attempt.run_id(),
		run_attempt.attempt_number(),
		"lane_decision",
		lane_snapshot.to_json(lane_decision.next_action, lane_decision.reason),
	)?;

	if lane_decision_blocks_automatic_execution(lane_decision.next_action) {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;

		return Ok(());
	}

	queue_child_exit_retry(
		context.retry_queue,
		context.state_store,
		context.workflow,
		ChildExitRetrySchedule {
			project_id: context.project.service_id(),
			issue_id,
			run_id: run_attempt.run_id(),
			attempt_number: run_attempt.attempt_number(),
			continuation_initial_issue_state,
			dispatch_mode,
			kind,
			attempt,
		},
	)
}

fn queue_child_exit_retry(
	retry_queue: &mut RetryQueue,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	schedule: ChildExitRetrySchedule<'_>,
) -> Result<()> {
	let attempt = schedule.attempt.max(1);
	let delay = retry_delay(schedule.kind, attempt, workflow);

	tracing::info!(
		issue_id = schedule.issue_id,
		retry_kind = ?schedule.kind,
		retry_attempt = attempt,
		retry_delay_ms = delay.as_millis(),
		"Queued retry after control-plane child exit."
	);

	let retry_ready_at_unix_epoch = OffsetDateTime::now_utc().unix_timestamp().saturating_add(
		i64::try_from((delay.as_millis().saturating_add(999)) / 1_000).unwrap_or(i64::MAX),
	);

	write_retry_schedule_for_run(
		state_store,
		schedule.issue_id,
		schedule.run_id,
		schedule.attempt_number,
		schedule.kind,
		retry_ready_at_unix_epoch,
	)?;

	if schedule.kind == RetryKind::Continuation {
		state_store.append_private_execution_event(
			schedule.project_id,
			schedule.issue_id,
			schedule.run_id,
			schedule.attempt_number,
			"continuation_lineage",
			json!({
				"schema": "decodex.continuation_lineage/1",
				"continuation_of_run_id": schedule.run_id,
				"source_attempt_number": schedule.attempt_number,
				"phase_cursor": "issue_private_evidence",
				"retry_budget_consumed": false,
				"retry_schedule_attempt": attempt,
				"continuation_initial_issue_state": schedule.continuation_initial_issue_state.as_deref(),
				"dispatch_mode": schedule.dispatch_mode.as_str(),
				"next_retry_kind": schedule.kind.as_str(),
			}),
		)?;
	}

	retry_queue.upsert(RetryEntry {
		issue_id: schedule.issue_id.to_owned(),
		#[cfg(test)]
		retry_project_slug: String::new(),
		continuation_initial_issue_state: schedule.continuation_initial_issue_state,
		dispatch_mode: schedule.dispatch_mode,
		kind: schedule.kind,
		attempt,
		ready_at: Instant::now() + delay,
	});

	Ok(())
}

fn recover_child_exit_phase_goal<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	issue_id: &str,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	exit_success: bool,
) -> Result<ChildExitPhaseGoalRecovery>
where
	T: IssueTracker,
{
	if exit_success {
		return Ok(ChildExitPhaseGoalRecovery::None);
	}

	let recovery = maybe_recover_child_exit_phase_goal_continuation(
		context,
		issue,
		child,
		initial_issue_state,
		dispatch_mode,
	)?;

	if matches!(recovery, ChildExitPhaseGoalRecovery::Terminalized) {
		clear_retry_schedule_and_release(context.retry_queue, context.state_store, issue_id)?;
	}

	Ok(recovery)
}

fn maybe_recover_child_exit_phase_goal_continuation<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
) -> Result<ChildExitPhaseGoalRecovery>
where
	T: IssueTracker,
{
	let worktree = child_exit_worktree_spec(context, issue)?;
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: initial_issue_state.to_owned(),
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode,
		attempt_number: child.attempt_number,
		run_id: child.run_id.to_owned(),
		retry_budget_base: 0,
	};
	let recovery = match recover_phase_goal_continuation(
		context.project,
		context.workflow,
		context.state_store,
		&issue_run,
		"child_exit_failed",
		Some("child_exit_failed"),
	) {
		Ok(recovery) => recovery,
		Err(error) if run_failure_requires_terminal_attention(&error) => {
			handle_failure(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&issue_run,
				&error,
			)?;

			return Ok(ChildExitPhaseGoalRecovery::Terminalized);
		},
		Err(error) => return Err(error),
	};

	if let Some(recovery) = &recovery {
		tracing::warn!(
			project_id = context.project.service_id(),
			issue_id = issue.id,
			issue = issue.identifier,
			run_id = child.run_id,
			attempt = child.attempt_number,
			source_phase = recovery.source_phase.as_str(),
			next_phase = recovery.next_phase.as_str(),
			"Recovered phase goal after child exit failure; scheduling continuation."
		);
	}

	Ok(recovery.map_or(ChildExitPhaseGoalRecovery::None, |recovery| {
		ChildExitPhaseGoalRecovery::Continuation(recovery)
	}))
}

fn child_exit_retry_retention_decision<T>(
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

fn child_exit_retry_budget_attempt_count<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
) -> Result<u32>
where
	T: IssueTracker,
{
	let state_attempts = context.state_store.retry_budget_attempt_count(&issue.id)?.max(1);
	let worktree = child_exit_worktree_spec(context, issue)?;
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(u32::try_from(state_attempts).unwrap_or(u32::MAX).max(1));
	};
	let marker_attempts = state::read_run_retry_budget_attempt_count(&worktree.path)?.unwrap_or(0);
	let marker_is_current_child =
		marker.run_id() == child.run_id && marker.attempt_number() == child.attempt_number;
	let marker_attempt_is_local = context.state_store.run_attempt(marker.run_id())?.is_some();
	let retry_budget_attempts =
		if marker_attempts > 0 && !marker_is_current_child && !marker_attempt_is_local {
			marker_attempts.saturating_add(state_attempts)
		} else {
			marker_attempts.max(state_attempts)
		};

	Ok(u32::try_from(retry_budget_attempts).unwrap_or(u32::MAX).max(1))
}

fn child_exit_retry_budget_limit<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
) -> Result<u32>
where
	T: IssueTracker,
{
	let max_attempts = context.workflow.frontmatter().execution().max_attempts();
	let worktree = child_exit_worktree_spec(context, issue)?;
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(max_attempts);
	};

	if marker.run_id() == child.run_id
		&& marker.attempt_number() == child.attempt_number
		&& marker.retry_kind() == Some(ARCHITECTURE_RECOVERY_RETRY_KIND)
	{
		return Ok(
			max_attempts.saturating_add(u32::try_from(ARCHITECTURE_RECOVERY_BUDGET).unwrap_or(0))
		);
	}

	Ok(max_attempts)
}

fn terminalize_exhausted_child_exit_retry<T>(
	context: ChildExitRetryContext<'_, T>,
	issue: TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	retry_budget_attempts: u32,
) -> Result<()>
where
	T: IssueTracker,
{
	apply_child_exit_terminal_failure_writeback(
		&context,
		&issue,
		child,
		initial_issue_state,
		dispatch_mode,
		i64::from(retry_budget_attempts),
	)?;
	clear_retry_schedule_and_release(context.retry_queue, context.state_store, child.issue_id)?;

	Ok(())
}

fn apply_child_exit_terminal_failure_writeback<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	child: ChildRunRef<'_>,
	initial_issue_state: &str,
	dispatch_mode: IssueDispatchMode,
	retry_budget_attempts: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let worktree = child_exit_worktree_spec(context, issue)?;
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: initial_issue_state.to_owned(),
		worktree,
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode,
		attempt_number: child.attempt_number,
		run_id: child.run_id.to_owned(),
		retry_budget_base: 0,
	};
	let worktree_path = relative_worktree_path(context.project, &issue_run.worktree);
	let error = if worktree_has_tracked_changes(&issue_run.worktree.path) {
		Report::new(RetainedPartialProgress {
			issue_identifier: issue.identifier.clone(),
			run_id: child.run_id.to_owned(),
			worktree_path: worktree_path.clone(),
			source_error_class: None,
		})
	} else {
		Report::msg(format!(
			"Daemon child `{}` for issue `{}` exited unsuccessfully after exhausting retry budget.",
			child.run_id, issue.identifier
		))
	};
	let privacy_classifier = configured_public_projection_privacy_classifier(context.project)?;
	let outcome = apply_terminal_failure_writeback(
		context.tracker,
		TerminalFailureWritebackRuntime {
			service_id: context.project.service_id(),
			state_store: Some(context.state_store),
			privacy_classifier: &privacy_classifier,
		},
		context.workflow,
		&issue_run,
		&worktree_path,
		false,
		&error,
	)?;

	if outcome.retry_guarded_by_state {
		write_terminal_guard_marker(
			&issue_run.worktree.path,
			&issue_run.run_id,
			issue_run.attempt_number,
		)?;

		context.state_store.update_run_status(&issue_run.run_id, TERMINAL_GUARDED_RUN_STATUS)?;
	}

	write_retry_budget_marker(
		&issue_run.worktree.path,
		&issue_run.run_id,
		issue_run.attempt_number,
		retry_budget_attempts,
	)?;

	tracing::warn!(
		project_id = context.project.service_id(),
		issue_id = issue.id,
		issue = issue.identifier,
		run_id = child.run_id,
		attempt = child.attempt_number,
		retry_budget_attempt = retry_budget_attempts,
		branch = issue_run.worktree.branch_name,
		worktree_path = %worktree_path,
		error_class = outcome.error_class,
		"Daemon child failed and now requires operator attention."
	);

	Ok(())
}

fn child_exit_worktree_spec<T>(
	context: &ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<WorktreeSpec>
where
	T: IssueTracker,
{
	if let Some(mapping) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(WorktreeSpec {
			branch_name: mapping.branch_name().to_owned(),
			issue_identifier: issue.identifier.clone(),
			path: mapping.worktree_path().to_path_buf(),
			reused_existing: true,
		});
	}

	let worktree_manager = WorktreeManager::new(
		context.project.service_id(),
		context.project.repo_root(),
		context.project.worktree_root(),
	);

	Ok(worktree_manager.plan_for_issue(&issue.identifier))
}

pub(crate) fn write_retry_schedule_for_run(
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	kind: RetryKind,
	retry_ready_at_unix_epoch: i64,
) -> Result<()> {
	let default_kind = match kind {
		RetryKind::Continuation => "continuation",
		RetryKind::Failure => "failure",
	};
	let retry_kind_label =
		preserved_retry_schedule_kind(state_store, issue_id, run_id, attempt_number, default_kind)?;

	if let Some(worktree) = state_store.worktree_for_issue(issue_id)? {
		state::write_run_retry_schedule(
			worktree.worktree_path(),
			run_id,
			attempt_number,
			&retry_kind_label,
			retry_ready_at_unix_epoch,
		)?;
	}

	Ok(())
}

fn preserved_retry_schedule_kind(
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_kind: &str,
) -> Result<String> {
	let Some(worktree) = state_store.worktree_for_issue(issue_id)? else {
		return Ok(default_kind.to_owned());
	};
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree.worktree_path())? else {
		return Ok(default_kind.to_owned());
	};

	if marker.run_id() == run_id
		&& marker.attempt_number() == attempt_number
		&& let Some(retry_kind) = marker.retry_kind()
	{
		return Ok(retry_kind.to_owned());
	}

	Ok(default_kind.to_owned())
}

pub(super) fn clear_retry_schedule_and_release(
	retry_queue: &mut RetryQueue,
	state_store: &StateStore,
	issue_id: &str,
) -> Result<()> {
	clear_worktree_retry_schedule(state_store, issue_id)?;

	retry_queue.release(issue_id);

	Ok(())
}

pub(crate) fn retry_delay(kind: RetryKind, attempt: u32, workflow: &WorkflowDocument) -> Duration {
	match kind {
		RetryKind::Continuation => Duration::from_millis(CONTINUATION_RETRY_DELAY_MS),
		RetryKind::Failure => {
			let exponent = attempt.saturating_sub(1).min(31);
			let multiplier = 1_u128 << exponent;
			let requested = u128::from(FAILURE_RETRY_BASE_DELAY_MS).saturating_mul(multiplier);
			let capped = requested
				.min(u128::from(workflow.frontmatter().execution().max_retry_backoff_ms()));

			Duration::from_millis(capped as u64)
		},
	}
}
