use time::OffsetDateTime;

use crate::{
	orchestrator::{
		OperatorConnectorBackoffStatus, StateStore, TrackerConnectorBackoff,
		entrypoints_tracker_backoff::{status, status::ConnectorBackoffStatusParts},
	},
	prelude::Result,
	state::{ConnectorBackoff, ConnectorBackoffInput},
};

pub(crate) fn active_stored_tracker_backoff_status(
	state_store: &StateStore,
	project_id: &str,
) -> Result<Option<OperatorConnectorBackoffStatus>> {
	let Some(backoff) = state_store.connector_backoff(project_id, "linear")? else {
		return Ok(None);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	if backoff.reset_unix_epoch() <= now_unix_epoch {
		state_store.clear_connector_backoff(project_id, "linear")?;

		return Ok(None);
	}

	Ok(Some(connector_backoff_record_to_operator_status(&backoff, now_unix_epoch)))
}

pub(crate) fn active_stored_tracker_backoff_status_best_effort(
	state_store: &StateStore,
	project_id: &str,
) -> Option<OperatorConnectorBackoffStatus> {
	match active_stored_tracker_backoff_status(state_store, project_id) {
		Ok(status) => status,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = project_id,
				"Failed to read persisted tracker backoff; sensitive runtime details were withheld."
			);

			None
		},
	}
}

pub(crate) fn persist_tracker_backoff_state(
	state_store: &StateStore,
	project_id: &str,
	backoff: &TrackerConnectorBackoff,
) {
	if let Err(error) = state_store.upsert_connector_backoff(ConnectorBackoffInput {
		project_id,
		connector: "linear",
		sync_phase: backoff.sync_phase,
		quota_class: backoff.quota_class,
		reset_unix_epoch: backoff.reset_unix_epoch,
		reset_source: backoff.reset_source,
		warning: backoff.warning,
	}) {
		let _ = error;

		tracing::warn!(
			project_id = project_id,
			"Failed to persist tracker backoff; sensitive runtime details were withheld."
		);
	}
}

pub(crate) fn clear_tracker_backoff_state_best_effort(state_store: &StateStore, project_id: &str) {
	if let Err(error) = state_store.clear_connector_backoff(project_id, "linear") {
		let _ = error;

		tracing::warn!(
			project_id = project_id,
			"Failed to clear persisted tracker backoff; sensitive runtime details were withheld."
		);
	}
}

fn connector_backoff_record_to_operator_status(
	backoff: &ConnectorBackoff,
	now_unix_epoch: i64,
) -> OperatorConnectorBackoffStatus {
	status::operator_connector_backoff_status(
		ConnectorBackoffStatusParts {
			project_id: backoff.project_id(),
			connector: backoff.connector(),
			sync_phase: backoff.sync_phase(),
			quota_class: backoff.quota_class(),
			reset_unix_epoch: backoff.reset_unix_epoch(),
			reset_source: backoff.reset_source(),
			warning: backoff.warning(),
			next_action: status::connector_backoff_next_action(backoff.warning()),
		},
		now_unix_epoch,
	)
}
