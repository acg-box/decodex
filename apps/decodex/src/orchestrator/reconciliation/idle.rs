use std::time::Duration;

use crate::{
	agent,
	orchestrator::{
		RUN_LEASE_IDLE_TIMEOUT, RUN_OPERATION_REPO_GATE, Result, RunActivityMarker, RunAttempt,
		StateStore, WorktreeMapping, marker_process_is_alive,
	},
	state,
};

pub(in crate::orchestrator) fn stalled_idle_duration(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
	now_unix_epoch: i64,
) -> Result<Option<Duration>> {
	if !matches!(run_attempt.status(), "starting" | "running") {
		return Ok(None);
	}
	if stalled_reconciliation_deferred_by_marker(run_attempt, worktree_mapping)? {
		return Ok(None);
	}

	let Some(last_activity) =
		last_observed_run_activity_unix_epoch(state_store, run_attempt, worktree_mapping)?
	else {
		return Ok(None);
	};
	let Some(idle_for) = observed_idle_duration(last_activity, now_unix_epoch) else {
		return Ok(None);
	};
	let idle_timeout = run_lease_idle_timeout(run_attempt, worktree_mapping)?;

	if idle_for >= idle_timeout {
		return Ok(Some(idle_for));
	}

	Ok(None)
}

fn run_lease_idle_timeout(
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Duration> {
	let Some(worktree_mapping) = worktree_mapping else {
		return Ok(RUN_LEASE_IDLE_TIMEOUT);
	};
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_mapping.worktree_path())?
	else {
		return Ok(RUN_LEASE_IDLE_TIMEOUT);
	};

	if marker.run_id() != run_attempt.run_id()
		|| marker.attempt_number() != run_attempt.attempt_number()
	{
		return Ok(RUN_LEASE_IDLE_TIMEOUT);
	}

	Ok(agent::protocol_activity_idle_timeout(marker.protocol_activity(), RUN_LEASE_IDLE_TIMEOUT))
}

fn last_observed_run_activity_unix_epoch(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Option<i64>> {
	let state_store_activity = state_store.last_run_activity_unix_epoch(run_attempt.run_id())?;
	let worktree_activity = match worktree_mapping {
		Some(mapping) => state::read_run_activity_marker(
			mapping.worktree_path(),
			run_attempt.run_id(),
			run_attempt.attempt_number(),
		)?,
		None => None,
	};

	Ok(match (state_store_activity, worktree_activity) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(activity), None) | (None, Some(activity)) => Some(activity),
		(None, None) => None,
	})
}

pub(crate) fn stalled_protocol_idle_duration(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
	now_unix_epoch: i64,
) -> Result<Option<Duration>> {
	if stalled_reconciliation_deferred_by_marker(run_attempt, worktree_mapping)? {
		return Ok(None);
	}

	let Some(last_activity) =
		last_observed_protocol_activity_unix_epoch(state_store, run_attempt, worktree_mapping)?
	else {
		return Ok(None);
	};
	let Some(idle_for) = observed_idle_duration(last_activity, now_unix_epoch) else {
		return Ok(None);
	};

	if idle_for >= RUN_LEASE_IDLE_TIMEOUT {
		return Ok(Some(idle_for));
	}

	Ok(None)
}

fn last_observed_protocol_activity_unix_epoch(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Option<i64>> {
	let state_store_activity =
		state_store.last_protocol_activity_unix_epoch(run_attempt.run_id())?;
	let worktree_activity = match worktree_mapping {
		Some(mapping) => state::read_run_protocol_activity_marker(
			mapping.worktree_path(),
			run_attempt.run_id(),
			run_attempt.attempt_number(),
		)?,
		None => None,
	};

	Ok(match (state_store_activity, worktree_activity) {
		(Some(left), Some(right)) => Some(left.max(right)),
		(Some(activity), None) | (None, Some(activity)) => Some(activity),
		(None, None) => None,
	})
}

pub(in crate::orchestrator) fn observed_idle_duration(
	last_activity_unix_epoch: i64,
	now_unix_epoch: i64,
) -> Option<Duration> {
	now_unix_epoch
		.checked_sub(last_activity_unix_epoch)
		.and_then(|idle_seconds| u64::try_from(idle_seconds).ok())
		.map(Duration::from_secs)
}

fn stalled_reconciliation_deferred_by_marker(
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<bool> {
	let Some(marker) = current_run_activity_marker(run_attempt, worktree_mapping)? else {
		return Ok(false);
	};

	if marker.retry_kind().is_some() {
		return Ok(true);
	}

	Ok(marker.current_operation() == Some(RUN_OPERATION_REPO_GATE)
		&& marker_process_is_alive(&marker))
}

fn current_run_activity_marker(
	run_attempt: &RunAttempt,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<Option<RunActivityMarker>> {
	let Some(worktree_mapping) = worktree_mapping else {
		return Ok(None);
	};
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_mapping.worktree_path())?
	else {
		return Ok(None);
	};

	if marker.run_id() == run_attempt.run_id()
		&& marker.attempt_number() == run_attempt.attempt_number()
	{
		return Ok(Some(marker));
	}

	Ok(None)
}
