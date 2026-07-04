use crate::{orchestrator::status_run_projection, state::RunActivityMarker};

pub(crate) fn operator_queued_issue_marker_activity_at(
	marker: Option<&RunActivityMarker>,
) -> Option<String> {
	marker.and_then(RunActivityMarker::last_activity_unix_epoch).and_then(|unix_epoch| {
		status_run_projection::format_optional_unix_timestamp(Some(unix_epoch))
	})
}

pub(crate) fn operator_queued_issue_marker_progress_at(
	marker: Option<&RunActivityMarker>,
) -> Option<String> {
	marker.and_then(RunActivityMarker::last_progress_unix_epoch).and_then(|unix_epoch| {
		status_run_projection::format_optional_unix_timestamp(Some(unix_epoch))
	})
}
