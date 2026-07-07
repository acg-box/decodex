use crate::{
	config::ServiceConfig,
	orchestrator::{
		OperatorHistoryLedgerOutcome, OperatorLaneTerminalProjection, OperatorRunStatus,
		OperatorStatusSnapshot, status_history_ledger, status_history_projection::predicates,
		status_run_projection,
	},
	prelude::Result,
	state::{ReviewLifecycleRecord, StateStore},
};

pub(crate) fn hydrate_history_lanes_from_local_ledger(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<()> {
	for lane in &mut snapshot.history_lanes {
		let records =
			state_store.list_linear_execution_events(project.service_id(), &lane.issue_id)?;

		if records.is_empty() {
			lane.ledger_outcome = status_history_ledger::missing_history_ledger_outcome();

			continue;
		}

		let records = status_history_ledger::local_history_ledger_records(records);

		status_history_ledger::hydrate_history_lane_from_ledger_records(lane, &records);

		lane.ledger_outcome = status_history_ledger::operator_history_ledger_outcome(&records);
	}

	Ok(())
}

pub(crate) fn current_lane_terminal_projection_from_local_ledger(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &OperatorStatusSnapshot,
) -> Result<OperatorLaneTerminalProjection> {
	let mut projection = OperatorLaneTerminalProjection::default();

	for run in &snapshot.current_lanes {
		if let Some(outcome) =
			current_lane_terminal_projection_from_lifecycle_authority(project, state_store, run)?
		{
			projection
				.outcomes_by_issue_key
				.insert(status_run_projection::operator_run_group_key(run), outcome);

			continue;
		}

		let records =
			state_store.list_linear_execution_events(project.service_id(), &run.issue_id)?;

		if records.is_empty() {
			continue;
		}

		let records = status_history_ledger::local_history_ledger_records(records);
		let outcome = status_history_ledger::operator_history_ledger_outcome(&records);

		if predicates::history_ledger_outcome_is_terminal(&outcome) {
			projection
				.outcomes_by_issue_key
				.insert(status_run_projection::operator_run_group_key(run), outcome);
		}
	}

	Ok(projection)
}

fn current_lane_terminal_projection_from_lifecycle_authority(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
) -> Result<Option<OperatorHistoryLedgerOutcome>> {
	let Some(branch_name) = run.branch_name.as_deref() else {
		return Ok(None);
	};
	let Some(record) =
		state_store.review_lifecycle_record(project.service_id(), &run.issue_id, branch_name)?
	else {
		return Ok(None);
	};

	Ok(lifecycle_authority_terminal_outcome(&record))
}

fn lifecycle_authority_terminal_outcome(
	record: &ReviewLifecycleRecord,
) -> Option<OperatorHistoryLedgerOutcome> {
	if record.sequence() <= 0
		|| record.closeout_state() != "completed"
		|| record.cleanup_state() != "completed"
	{
		return None;
	}

	Some(OperatorHistoryLedgerOutcome {
		ledger_status: String::from("authority"),
		final_outcome: record.next_state().to_owned(),
		final_event_type: Some(record.transition().to_owned()),
		final_event_at: Some(record.decided_at().to_owned()),
		summary: Some(format!(
			"Lifecycle authority record {} completed closeout for PR {}.",
			record.sequence(),
			record.pr_url()
		)),
		pr_url: Some(record.pr_url().to_owned()),
		commit_sha: record.merge_commit().map(str::to_owned),
		branch: Some(record.branch_name().to_owned()),
		closeout_status: Some(String::from("completed")),
		needs_attention_reason: None,
		lifecycle_started_at: None,
		lifecycle_finished_at: Some(record.decided_at().to_owned()),
		lifecycle_elapsed_seconds: None,
		record_count: 1,
	})
}
