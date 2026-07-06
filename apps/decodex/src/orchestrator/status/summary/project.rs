use std::collections::HashSet;

use crate::{
	orchestrator::{
		self, OperatorRunStatus, OperatorStatusSnapshot,
		status_summary::{attention, cleanup, queue, run_state},
	},
	state::RUN_OPERATION_WAITING_EXTERNAL,
};

pub(super) fn refresh_operator_project_summary(
	snapshot: &mut OperatorStatusSnapshot,
	completed_state: Option<&str>,
) {
	let current_lane_count = snapshot.current_lanes.len();
	let running_lane_count = snapshot
		.current_lanes
		.iter()
		.filter(|run| run_state::operator_run_counts_as_running(run))
		.count();
	let queued_candidate_count = snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| queue::queued_candidate_counts_as_waiting_intake(candidate))
		.count();
	let post_review_lane_count =
		snapshot.post_review_lanes.iter().filter(|lane| !lane.shadowed_by_current_lane).count();
	let retained_worktree_count = orchestrator::rendered_recovery_worktrees(snapshot).len();
	let waiting_lane_count = project_waiting_lane_count(snapshot);
	let attention_count = attention::project_attention_count(snapshot, completed_state);
	let cleanup_blocked_count = cleanup::project_cleanup_blocked_count(snapshot);
	let cleanup_pending_count = cleanup::project_cleanup_pending_count(snapshot);
	let connector_state = project_connector_state(snapshot);
	let last_activity_at = project_last_activity_at(snapshot);
	let warning_count = snapshot.warnings.len();

	if let Some(project_status) = snapshot.projects.first_mut() {
		project_status.current_lane_count = current_lane_count;
		project_status.running_lane_count = running_lane_count;
		project_status.queued_candidate_count = queued_candidate_count;
		project_status.post_review_lane_count = post_review_lane_count;
		project_status.retained_worktree_count = retained_worktree_count;
		project_status.waiting_lane_count = waiting_lane_count;
		project_status.attention_count = attention_count;
		project_status.cleanup_blocked_count = cleanup_blocked_count;
		project_status.cleanup_pending_count = cleanup_pending_count;
		project_status.connector_state = connector_state;
		project_status.last_activity_at = last_activity_at;
		project_status.warning_count = warning_count;
	}
}

fn project_waiting_lane_count(snapshot: &OperatorStatusSnapshot) -> usize {
	let waiting_run_count = project_summary_runs(snapshot)
		.into_iter()
		.filter(|run| operator_run_counts_as_project_waiting(run))
		.map(|run| run.run_id.as_str())
		.collect::<HashSet<_>>()
		.len();
	let queued_waiting = snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| candidate.classification == "waiting")
		.count();
	let review_waiting = snapshot
		.post_review_lanes
		.iter()
		.filter(|lane| !lane.shadowed_by_current_lane && lane.classification == "wait_for_review")
		.count();

	waiting_run_count + queued_waiting + review_waiting
}

fn project_summary_runs(snapshot: &OperatorStatusSnapshot) -> Vec<&OperatorRunStatus> {
	let mut runs = snapshot.current_lanes.iter().collect::<Vec<_>>();

	runs.extend(snapshot.history_lanes.iter().map(|lane| &lane.latest_run));

	runs
}

fn operator_run_counts_as_project_waiting(run: &OperatorRunStatus) -> bool {
	if run_state::operator_run_counts_as_attention(run) {
		return false;
	}
	if matches!(run.phase.as_str(), "retry_backoff" | "waiting_continuation") {
		return true;
	}
	if run.current_operation == RUN_OPERATION_WAITING_EXTERNAL {
		return true;
	}

	matches!(run.wait_reason.as_deref(), Some("approval_or_user_input" | "protocol_idleness"))
}

fn project_connector_state(snapshot: &OperatorStatusSnapshot) -> String {
	if !snapshot.connector_backoffs.is_empty()
		|| orchestrator::snapshot_warnings_include_tracker_backoff(snapshot)
	{
		return String::from("backoff");
	}
	if !snapshot.warnings.is_empty() {
		return String::from("degraded");
	}
	if project_summary_runs(snapshot)
		.into_iter()
		.any(|run| run.phase == "retry_backoff" || run.next_retry_at.is_some())
	{
		return String::from("backoff");
	}

	String::from("ok")
}

fn project_last_activity_at(snapshot: &OperatorStatusSnapshot) -> Option<String> {
	snapshot
		.current_lanes
		.iter()
		.chain(snapshot.recent_runs.iter())
		.chain(snapshot.history_lanes.iter().map(|lane| &lane.latest_run))
		.flat_map(|run| {
			[
				run.last_progress_at.as_deref(),
				run.last_run_activity_at.as_deref(),
				run.last_protocol_activity_at.as_deref(),
				run.last_event_at.as_deref(),
				Some(run.updated_at.as_str()),
			]
		})
		.flatten()
		.max()
		.map(str::to_owned)
}
