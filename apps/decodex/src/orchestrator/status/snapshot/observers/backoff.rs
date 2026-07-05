use time::OffsetDateTime;

use crate::{
	orchestrator::status::{
		self, LiveOperatorStatusObserverContext, OperatorConnectorBackoffStatus,
		OperatorStatusSnapshot, ServiceConfig, StateStore, TrackerConnectorBackoff,
		TrackerObserverOutcome,
	},
	prelude::Result,
};

pub(crate) fn pause_operator_snapshot_for_stored_tracker_backoff<T>(
	context: &LiveOperatorStatusObserverContext<'_, T>,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<bool> {
	let Some(backoff) = status::active_stored_tracker_backoff_status(
		context.state_store,
		context.project.service_id(),
	)?
	else {
		return Ok(false);
	};

	add_tracker_backoff_to_operator_snapshot(snapshot, &backoff);

	Ok(true)
}

pub(crate) fn apply_tracker_observer_outcome(
	outcome: TrackerObserverOutcome,
	snapshot: &mut OperatorStatusSnapshot,
	state_store: &StateStore,
	project: &ServiceConfig,
	unavailable_warning: &'static str,
) -> bool {
	match outcome {
		TrackerObserverOutcome::Ok => false,
		TrackerObserverOutcome::Unavailable => {
			add_operator_snapshot_warning(snapshot, unavailable_warning);

			false
		},
		TrackerObserverOutcome::Backoff(backoff) => {
			pause_operator_snapshot_for_tracker_backoff(snapshot, state_store, project, &backoff);

			true
		},
	}
}

pub(crate) fn pause_operator_snapshot_for_tracker_backoff(
	snapshot: &mut OperatorStatusSnapshot,
	state_store: &StateStore,
	project: &ServiceConfig,
	backoff: &TrackerConnectorBackoff,
) {
	status::persist_tracker_backoff_state(state_store, project.service_id(), backoff);

	let backoff = backoff
		.to_operator_status(project.service_id(), OffsetDateTime::now_utc().unix_timestamp());

	add_tracker_backoff_to_operator_snapshot(snapshot, &backoff);
}

pub(crate) fn add_tracker_backoff_to_operator_snapshot(
	snapshot: &mut OperatorStatusSnapshot,
	backoff: &OperatorConnectorBackoffStatus,
) {
	add_operator_snapshot_warning(snapshot, &backoff.warning);

	if !snapshot.connector_backoffs.iter().any(|existing| {
		existing.project_id == backoff.project_id && existing.connector == backoff.connector
	}) {
		snapshot.connector_backoffs.push(backoff.clone());
	}
}

pub(crate) fn add_operator_snapshot_warning(snapshot: &mut OperatorStatusSnapshot, warning: &str) {
	if !snapshot.warnings.iter().any(|existing| existing == warning) {
		snapshot.warnings.push(warning.to_owned());
	}
}
