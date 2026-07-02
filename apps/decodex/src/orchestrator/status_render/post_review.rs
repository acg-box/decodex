use crate::orchestrator::{OperatorStatusSnapshot, status_render::activity};

pub(super) fn append_rendered_post_review_lanes(
	output: &mut String,
	snapshot: &OperatorStatusSnapshot,
) {
	if snapshot.post_review_lanes.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for lane in &snapshot.post_review_lanes {
		let loop_status = activity::render_loop_status_summary(lane.loop_status.as_ref());
		let loop_review = activity::render_loop_review_summary(lane.loop_status.as_ref());
		let loop_architecture_recovery =
			activity::render_loop_architecture_recovery_summary(lane.loop_status.as_ref());
		let loop_boundary = activity::render_loop_boundary_summary(lane.loop_status.as_ref());

		output.push_str(&format!(
			"- issue_id: {}\n  issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  shadowed_by_current_lane: {}\n  branch: {}\n  worktree_path: {}\n  pr_url: {}\n  pr_head_sha: {}\n  pr_state: {}\n  review_decision: {}\n  mergeable: {}\n  check_state: {}\n  unresolved_review_threads: {}\n  readback_warning: {}\n  readback_root_cause: {}\n  loop_status: {}\n  loop_review: {}\n  loop_architecture_recovery: {}\n  loop_boundary: {}\n",
			lane.issue_id,
			lane.issue_identifier,
			lane.issue_state,
			lane.classification,
			lane.reason,
			if lane.shadowed_by_current_lane { "yes" } else { "no" },
			lane.branch_name,
			lane.worktree_path,
			lane.pr_url.as_deref().unwrap_or("none"),
			lane.pr_head_sha.as_deref().unwrap_or("none"),
			lane.pr_state.as_deref().unwrap_or("none"),
			lane.review_decision.as_deref().unwrap_or("none"),
			lane.mergeable.as_deref().unwrap_or("none"),
			lane.check_state.as_deref().unwrap_or("none"),
			lane.unresolved_review_threads
				.map_or_else(|| String::from("none"), |value| value.to_string()),
			lane.readback_warning.as_deref().unwrap_or("none"),
			lane.readback_root_cause.as_deref().unwrap_or("none"),
			loop_status,
			loop_review,
			loop_architecture_recovery,
			loop_boundary
		));
	}
}
