use crate::orchestrator::{
	RunAttempt, TrackerIssue,
	daemon_retry::{
		self, CONTINUATION_PENDING_RUN_STATUS, ChildExitPhaseGoalRecovery, ChildExitRetryContext,
		ChildExitRetrySchedule, ChildRunRef, ExitStatus, Instant, IssueDispatchMode, IssueTracker,
		LaneDecisionSnapshot, OffsetDateTime, Result, RetryEntry, RetryEntryLifecycle,
		RetryEntryRetentionDecision, RetryKind, RetryQueue, StateStore, WorkflowDocument,
	},
};

pub(crate) fn schedule_retry_after_child_exit<T>(
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
	let Some(run_attempt) = active_child_exit_run_attempt(&mut context, child, exit_status)? else {
		return Ok(());
	};
	let issue_id = run_attempt.issue_id();
	let Some(issue) = refreshed_child_exit_issue(&mut context, issue_id)? else {
		return Ok(());
	};
	let continuation_pending =
		exit_status.success() && run_attempt.status() == CONTINUATION_PENDING_RUN_STATUS;

	if !exit_status.success() && run_attempt.status() != "failed" {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;

		return Ok(());
	}

	let retention_decision = daemon_retry::child_exit_retry_retention_decision(
		&context,
		&issue,
		initial_issue_state,
		RetryEntryLifecycle::for_dispatch_mode(dispatch_mode),
		continuation_pending,
	)?;

	if retention_decision == RetryEntryRetentionDecision::Drop {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;

		return Ok(());
	}

	let recovered_phase_goal_continuation = match daemon_retry::recover_child_exit_phase_goal(
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
		continuation_child_exit_retry_plan(&run_attempt, initial_issue_state)
	} else if recovered_phase_goal_continuation.is_some() {
		context
			.state_store
			.update_run_status(run_attempt.run_id(), CONTINUATION_PENDING_RUN_STATUS)?;

		continuation_child_exit_retry_plan(&run_attempt, initial_issue_state)
	} else if exit_status.success() {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;

		return Ok(());
	} else {
		let retry_budget_attempts =
			daemon_retry::child_exit_retry_budget_attempt_count(&context, &issue, child)?;
		let retry_budget_limit =
			daemon_retry::child_exit_retry_budget_limit(&context, &issue, child)?;

		if retry_budget_attempts >= retry_budget_limit {
			return daemon_retry::terminalize_exhausted_child_exit_retry(
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

	if !child_exit_lane_decision_permits_retry(
		&mut context,
		&issue,
		&run_attempt,
		dispatch_mode,
		kind,
	)? {
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

pub(crate) fn queue_child_exit_retry(
	retry_queue: &mut RetryQueue,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	schedule: ChildExitRetrySchedule<'_>,
) -> Result<()> {
	let attempt = schedule.attempt.max(1);
	let delay = daemon_retry::retry_delay(schedule.kind, attempt, workflow);

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

	daemon_retry::write_retry_schedule_for_run(
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
			daemon_retry::json!({
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
		lifecycle: RetryEntryLifecycle::for_dispatch_mode(schedule.dispatch_mode),
		dispatch_mode: schedule.dispatch_mode,
		kind: schedule.kind,
		attempt,
		ready_at: Instant::now() + delay,
	});

	Ok(())
}

fn continuation_child_exit_retry_plan(
	run_attempt: &RunAttempt,
	initial_issue_state: &str,
) -> (RetryKind, u32, Option<String>) {
	(
		RetryKind::Continuation,
		u32::try_from(run_attempt.attempt_number()).unwrap_or(u32::MAX).max(1),
		Some(initial_issue_state.to_owned()),
	)
}

fn active_child_exit_run_attempt<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	child: ChildRunRef<'_>,
	exit_status: ExitStatus,
) -> Result<Option<RunAttempt>>
where
	T: IssueTracker,
{
	let Some(run_attempt) =
		daemon_retry::resolve_child_exit_run_attempt(context.state_store, child)?
	else {
		tracing::debug!(
			issue_id = child.issue_id,
			run_id = child.run_id,
			attempt = child.attempt_number,
			"Daemon child exited without a matching recorded run attempt; skipping retry scheduling."
		);

		return Ok(None);
	};

	if !exit_status.success() {
		daemon_retry::mark_run_attempt_if_active(
			context.state_store,
			run_attempt.run_id(),
			"failed",
		)?;
	}

	let Some(run_attempt) = context.state_store.run_attempt(run_attempt.run_id())? else {
		return Ok(None);
	};

	if daemon_retry::superseded_run_disposition(context.state_store, &run_attempt)?.is_some() {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			child.issue_id,
		)?;

		return Ok(None);
	}

	Ok(Some(run_attempt))
}

fn refreshed_child_exit_issue<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	issue_id: &str,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	let issue = daemon_retry::refresh_issue(context.tracker, issue_id)?;

	if issue.is_none() {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue_id,
		)?;
	}

	Ok(issue)
}

fn child_exit_lane_decision_permits_retry<T>(
	context: &mut ChildExitRetryContext<'_, T>,
	issue: &TrackerIssue,
	run_attempt: &RunAttempt,
	dispatch_mode: IssueDispatchMode,
	kind: RetryKind,
) -> Result<bool>
where
	T: IssueTracker,
{
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
	let lane_decision = daemon_retry::decide_lane_next_action(&lane_snapshot);

	context.state_store.append_private_execution_event(
		context.project.service_id(),
		issue.id.as_str(),
		run_attempt.run_id(),
		run_attempt.attempt_number(),
		"lane_decision",
		lane_snapshot.to_json(lane_decision.next_action, lane_decision.reason),
	)?;

	let retry_permitted = !lane_decision.blocks_automatic_execution()
		&& lane_decision.permits_child_exit_retry_kind(kind);

	if !retry_permitted {
		daemon_retry::clear_retry_schedule_and_release(
			context.retry_queue,
			context.state_store,
			issue.id.as_str(),
		)?;
	}

	Ok(retry_permitted)
}
