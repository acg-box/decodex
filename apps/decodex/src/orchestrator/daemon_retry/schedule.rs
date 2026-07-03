use std::time::Duration;

use crate::{
	orchestrator::{
		CONTINUATION_RETRY_DELAY_MS, FAILURE_RETRY_BASE_DELAY_MS, Result, RetryKind, RetryQueue,
		StateStore, WorkflowDocument, clear_worktree_retry_schedule,
	},
	state,
};

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

pub(in crate::orchestrator) fn clear_retry_schedule_and_release(
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
