use crate::orchestrator::{
	self, OperatorProjectStatus, OperatorStatusSnapshot, ProjectRegistration,
};

pub(crate) fn hydrate_project_status_from_local_snapshot(
	project_status: &mut OperatorProjectStatus,
	project_snapshot: &OperatorStatusSnapshot,
) {
	if let Some(local_status) = project_snapshot.projects.first() {
		hydrate_project_status_from_registered_status(project_status, local_status);
	} else {
		project_status.current_lane_count = project_snapshot.current_lanes.len();
		project_status.running_lane_count = project_snapshot
			.current_lanes
			.iter()
			.filter(|run| orchestrator::operator_run_counts_as_running(run))
			.count();
	}
}

pub(crate) fn hydrate_project_status_from_registered_status(
	project_status: &mut OperatorProjectStatus,
	local_status: &OperatorProjectStatus,
) {
	project_status.current_lane_count = local_status.current_lane_count;
	project_status.running_lane_count = local_status.running_lane_count;
	project_status.retained_worktree_count = local_status.retained_worktree_count;
	project_status.waiting_lane_count = local_status.waiting_lane_count;
	project_status.attention_count = local_status.attention_count;
	project_status.cleanup_blocked_count = local_status.cleanup_blocked_count;
	project_status.cleanup_pending_count = local_status.cleanup_pending_count;
	project_status.last_activity_at = local_status.last_activity_at.clone();
	project_status.warning_count =
		project_status.warning_count.saturating_add(local_status.warning_count);
}

pub(crate) fn complete_project_status(
	project: &ProjectRegistration,
	mut status: OperatorProjectStatus,
) -> OperatorProjectStatus {
	status.config_path = project.config_path().display().to_string();
	status.enabled = project.enabled();

	status
}
