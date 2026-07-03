use std::time::Instant;

use color_eyre::Report;
use time::OffsetDateTime;

use crate::orchestrator::{self, OperatorConnectorBackoffStatus, ProjectDaemonRuntime, StateStore};

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn tracker_backoff_active(
	runtime: &mut ProjectDaemonRuntime,
	now: Instant,
) -> bool {
	if runtime.tracker_backoff.as_ref().is_some_and(|backoff| backoff.until > now) {
		return true;
	}

	runtime.tracker_backoff = None;

	false
}

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn remember_tracker_backoff(
	runtime: &mut ProjectDaemonRuntime,
	state_store: &StateStore,
	project_id: &str,
	error: &Report,
	now: Instant,
	sync_phase: &'static str,
) -> Option<OperatorConnectorBackoffStatus> {
	let backoff = orchestrator::tracker_connector_backoff(error, now, sync_phase)?;
	let status = backoff.to_operator_status(project_id, OffsetDateTime::now_utc().unix_timestamp());

	orchestrator::persist_tracker_backoff_state(state_store, project_id, &backoff);

	runtime.tracker_backoff = Some(backoff);

	Some(status)
}
