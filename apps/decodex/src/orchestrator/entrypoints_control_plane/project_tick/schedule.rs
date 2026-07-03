use std::time::Instant;

use crate::orchestrator::{
	LINEAR_CONTROL_PLANE_POLL_INTERVAL, OperatorLinearScanRequest, ProjectDaemonRuntime,
};

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn linear_scan_due(
	project_id: &str,
	runtime: &ProjectDaemonRuntime,
	linear_scan_requests: &[OperatorLinearScanRequest],
	now: Instant,
) -> bool {
	if linear_scan_requested(project_id, linear_scan_requests) {
		return true;
	}

	runtime.next_linear_scan_at.is_none_or(|next_scan_at| now >= next_scan_at)
}

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn remember_next_linear_scan(
	runtime: &mut ProjectDaemonRuntime,
	now: Instant,
) {
	runtime.next_linear_scan_at = Some(now + LINEAR_CONTROL_PLANE_POLL_INTERVAL);
}

pub(in crate::orchestrator::entrypoints_control_plane::project_tick) fn linear_scan_requested(
	project_id: &str,
	linear_scan_requests: &[OperatorLinearScanRequest],
) -> bool {
	linear_scan_requests.iter().any(|request| {
		request
			.project_id
			.as_deref()
			.is_none_or(|requested_project_id| requested_project_id == project_id)
	})
}
