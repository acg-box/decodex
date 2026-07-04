use crate::{
	orchestrator::execution_failure::{
		IssueRunPlan, IssueTracker, Result, TerminalFailureWritebackRuntime,
		terminal_writeback::{PreparedTerminalFailureWriteback, TerminalFailureEventRecordStatus},
	},
	tracker::records,
};

pub(crate) fn record_terminal_failure_writeback_event<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	issue_run: &IssueRunPlan,
	writeback: &PreparedTerminalFailureWriteback,
) -> Result<TerminalFailureEventRecordStatus>
where
	T: IssueTracker,
{
	let event_status = if let Some(state_store) = runtime.state_store {
		if !state_store.record_linear_execution_event(&writeback.projection.record)? {
			return Ok(TerminalFailureEventRecordStatus::Duplicate);
		}

		TerminalFailureEventRecordStatus::Recorded
	} else {
		TerminalFailureEventRecordStatus::NoLocalStore
	};

	if remote_terminal_failure_writeback_exists(
		tracker,
		runtime,
		issue_run,
		writeback,
		event_status,
	)? {
		return Ok(TerminalFailureEventRecordStatus::Duplicate);
	}

	Ok(event_status)
}

pub(crate) fn forget_terminal_failure_writeback_event(
	runtime: TerminalFailureWritebackRuntime<'_>,
	event_status: TerminalFailureEventRecordStatus,
	writeback: &PreparedTerminalFailureWriteback,
) -> Result<()> {
	if event_status == TerminalFailureEventRecordStatus::Recorded
		&& let Some(state_store) = runtime.state_store
	{
		state_store.forget_linear_execution_event(&writeback.projection.record.idempotency_key)?;
	}

	Ok(())
}

fn remote_terminal_failure_writeback_exists<T>(
	tracker: &T,
	runtime: TerminalFailureWritebackRuntime<'_>,
	issue_run: &IssueRunPlan,
	writeback: &PreparedTerminalFailureWriteback,
	event_status: TerminalFailureEventRecordStatus,
) -> Result<bool>
where
	T: IssueTracker,
{
	let comments = match tracker.list_comments(&issue_run.issue.id) {
		Ok(comments) => comments,
		Err(error) => {
			forget_terminal_failure_writeback_event(runtime, event_status, writeback)?;

			return Err(error);
		},
	};

	if !records::has_linear_execution_event_record(
		&comments,
		&writeback.projection.record.service_id,
		&writeback.projection.record.issue_id,
		&writeback.projection.record.idempotency_key,
	) {
		return Ok(false);
	}

	tracing::debug!(
		service_id = writeback.projection.record.service_id,
		issue_id = issue_run.issue.id,
		issue = issue_run.issue.identifier,
		run_id = issue_run.run_id,
		attempt = issue_run.attempt_number,
		event_type = writeback.projection.record.event_type,
		"Skipping terminal failure writeback already present in remote Linear ledger."
	);

	Ok(true)
}
