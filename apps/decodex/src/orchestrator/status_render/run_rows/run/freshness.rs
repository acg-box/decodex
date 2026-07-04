use crate::orchestrator::{self, OperatorRunStatus};

pub(in crate::orchestrator::status_render::run_rows::run) fn operator_run_freshness(
	run: &OperatorRunStatus,
) -> (&'static str, &str) {
	if orchestrator::operator_run_counts_as_current_lane(run) {
		if let Some(timestamp) = run.last_run_activity_at.as_deref() {
			return ("last_run_activity_at", timestamp);
		}
		if let Some(timestamp) = run.last_progress_at.as_deref() {
			return ("last_progress_at", timestamp);
		}
		if let Some(timestamp) = run.last_protocol_activity_at.as_deref() {
			return ("last_protocol_activity_at", timestamp);
		}

		return ("none", "none");
	}

	("updated_at", run.updated_at.as_str())
}
