use crate::orchestrator::daemon_retry::{
	self, ChildExitRetrySchedule, Instant, OffsetDateTime, Result, RetryEntry, RetryEntryLifecycle,
	RetryKind, RetryQueue, StateStore, WorkflowDocument,
};

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
