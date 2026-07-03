use std::path::Path;

use crate::{
	agent::app_server::{
		AppServerRunRequest, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_APP_SERVER_PREFLIGHT,
	},
	state::{self, ProtocolActivityMarker},
};

pub(in crate::agent::app_server) fn write_activity_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
) {
	if let Err(error) = state::write_run_operation_marker(
		marker_path,
		run_id,
		attempt_number,
		RUN_OPERATION_AGENT_RUN,
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree activity marker."
		);
	}
}

pub(in crate::agent::app_server) fn write_activity_marker_best_effort_for_request(
	request: &AppServerRunRequest<'_>,
) {
	if let Some(marker_path) = request.activity_marker_path.as_ref() {
		write_activity_marker_best_effort(marker_path, &request.run_id, request.attempt_number);
	}
}

pub(in crate::agent::app_server) fn write_capability_preflight_marker_best_effort(
	request: &AppServerRunRequest<'_>,
) {
	if let Some(marker_path) = request.activity_marker_path.as_ref()
		&& let Err(error) = state::write_run_operation_marker(
			marker_path,
			&request.run_id,
			request.attempt_number,
			RUN_OPERATION_APP_SERVER_PREFLIGHT,
		) {
		tracing::warn!(
			?error,
			run_id = request.run_id,
			attempt_number = request.attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree app-server preflight marker."
		);
	}
}

pub(in crate::agent::app_server) fn write_protocol_activity_marker_best_effort(
	marker_path: &Path,
	activity: &ProtocolActivityMarker<'_>,
) {
	if let Err(error) = state::write_run_protocol_activity_marker(marker_path, activity) {
		tracing::warn!(
			?error,
			run_id = activity.run_id,
			attempt_number = activity.attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree protocol-activity marker."
		);
	}
}
