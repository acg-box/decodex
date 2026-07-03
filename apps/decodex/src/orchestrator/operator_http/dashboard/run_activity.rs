use std::sync::{
	Arc,
	mpsc::{Receiver, RecvTimeoutError},
};

use serde_json::Value;
use time::OffsetDateTime;

use crate::orchestrator::{
	self, DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
	operator_http::{
		self, DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS, DashboardBroadcastEvent,
		DashboardEventHub, DashboardRunActivityEvent, OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL,
		OperatorCodexAccountControlStatus, OperatorRunStatus, Result, ServiceConfig, StateStore,
		TcpStream, WorkflowDocument, dashboard::framing, types::DashboardClientSubscription,
	},
};

pub(crate) fn run_operator_run_activity_websocket_broadcasts(
	state_store: Arc<StateStore>,
	dashboard_events: DashboardEventHub,
	shutdown_rx: Receiver<()>,
) {
	let mut last_fingerprint: Option<Vec<u8>> = None;

	loop {
		match shutdown_rx.recv_timeout(OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL) {
			Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
			Err(RecvTimeoutError::Timeout) => {},
		}

		if !dashboard_events.has_clients() {
			last_fingerprint = None;

			continue;
		}

		match build_operator_run_activity_event(&state_store) {
			Ok(event) => {
				if last_fingerprint.as_deref() == Some(event.fingerprint.as_slice()) {
					continue;
				}

				dashboard_events.broadcast(event.event.event_type, event.event.payload);

				last_fingerprint = Some(event.fingerprint);
			},
			Err(error) => {
				tracing::warn!(?error, "Skipped dashboard run activity event.");
			},
		}
	}
}

pub(crate) fn build_operator_run_activity_event(
	state_store: &StateStore,
) -> Result<DashboardRunActivityEvent> {
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let account_control = operator_http::global_codex_account_control_status();
	let mut current_lanes = Vec::new();

	for registration in state_store.list_projects()? {
		let project = match ServiceConfig::from_path(registration.config_path()) {
			Ok(project) => project,
			Err(error) => {
				tracing::debug!(
					project_id = registration.service_id(),
					config_path = %registration.config_path().display(),
					?error,
					"Skipped dashboard run activity for an unreadable registered project."
				);

				continue;
			},
		};
		let workflow = match WorkflowDocument::from_path(project.workflow_path()) {
			Ok(workflow) => workflow,
			Err(error) => {
				tracing::debug!(
					project_id = project.service_id(),
					workflow_path = %project.workflow_path().display(),
					?error,
					"Skipped dashboard run activity for a project with an unreadable workflow."
				);

				continue;
			},
		};
		let project_snapshot = orchestrator::build_operator_state_snapshot_without_live_observers(
			&project,
			&workflow,
			state_store,
			DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT,
		)?;

		if project_snapshot.current_lanes.is_empty() {
			continue;
		}

		current_lanes.extend(project_snapshot.current_lanes);
	}

	let fingerprint_payload =
		dashboard_run_activity_fingerprint_payload(&account_control, &current_lanes)?;
	let fingerprint = serde_json::to_vec(&fingerprint_payload)?;
	let presentation = operator_http::operator_snapshot_presentation_value(&current_lanes)?;
	let payload = operator_http::json!({
		"emittedAtUnixEpoch": now_unix_epoch,
		"accountControl": &account_control,
		"currentLanes": &current_lanes,
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
		"presentation": &presentation,
	});

	Ok(DashboardRunActivityEvent {
		fingerprint,
		event: DashboardBroadcastEvent { event_type: "runActivity", payload },
	})
}

pub(crate) fn strip_dashboard_run_activity_volatile_fields(value: &mut Value) {
	match value {
		Value::Object(object) => {
			for field in DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS {
				object.remove(*field);
			}
			for child in object.values_mut() {
				strip_dashboard_run_activity_volatile_fields(child);
			}
		},
		Value::Array(values) =>
			for child in values {
				strip_dashboard_run_activity_volatile_fields(child);
			},
		_ => {},
	}
}

pub(crate) fn dashboard_run_activity_fingerprint_payload(
	account_control: &OperatorCodexAccountControlStatus,
	current_lanes: &[OperatorRunStatus],
) -> Result<Value> {
	let mut fingerprint_payload = operator_http::json!({
		"accountControl": account_control,
		"currentLanes": current_lanes,
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
		"presentation": operator_http::operator_snapshot_presentation_value(current_lanes)?,
	});

	strip_dashboard_run_activity_volatile_fields(&mut fingerprint_payload);

	Ok(fingerprint_payload)
}

pub(crate) fn dashboard_run_activity_event_has_current_lanes(
	event: &DashboardBroadcastEvent,
) -> bool {
	event.payload.get("currentLanes").and_then(Value::as_array).is_some_and(|runs| !runs.is_empty())
}

pub(crate) fn write_cached_dashboard_run_activity_event(
	stream: &mut TcpStream,
	dashboard_events: &DashboardEventHub,
	subscription: &DashboardClientSubscription,
) {
	match dashboard_events.cached_run_activity_event(subscription) {
		Some(event) if dashboard_run_activity_event_has_current_lanes(&event) => {
			if let Err(error) =
				framing::write_dashboard_websocket_event(stream, event.event_type, &event.payload)
			{
				tracing::warn!(
					?error,
					"Skipped cached dashboard run activity snapshot for a WebSocket client."
				);
			}
		},
		Some(_) | None => {},
	}
}
