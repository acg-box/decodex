use crate::{
	config::ServiceConfig,
	orchestrator::{
		OperatorLaneTerminalProjection, OperatorStatusSnapshot, status_history_ledger,
		status_history_projection::predicates, status_run_projection,
	},
	prelude::Result,
	state::StateStore,
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
