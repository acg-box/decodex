use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{
	config::ServiceConfig, orchestrator::OperatorRunStatus, prelude::Result, state::StateStore,
};

pub(super) fn lane_control_operator_context(run: &OperatorRunStatus) -> Value {
	let control_capability = run.control_capability.as_ref().map(|capability| {
		serde_json::json!({
			"project_id": capability.project_id.as_str(),
			"issue_id": capability.issue_id.as_str(),
			"run_id": capability.run_id.as_str(),
			"attempt_number": capability.attempt_number,
			"thread_id": capability.thread_id.as_deref(),
			"turn_id": capability.turn_id.as_deref(),
			"transport": capability.transport.as_str(),
			"channel_path": capability.channel_path.as_str(),
			"status": capability.status.as_str(),
			"published_at": capability.published_at.as_str(),
			"updated_at": capability.updated_at.as_str(),
		})
	});

	serde_json::json!({
		"status": run.status.as_str(),
		"attempt_status": run.attempt_status.as_str(),
		"phase": run.phase.as_str(),
		"wait_reason": run.wait_reason.as_deref(),
		"current_operation": run.current_operation.as_str(),
		"run_lease": run.run_lease,
		"queue_lease_state": run.queue_lease_state.as_str(),
		"execution_liveness": run.execution_liveness.as_str(),
		"ownership_state": run.ownership_state.as_str(),
		"liveness_state": run.liveness_state.as_str(),
		"policy_state": run.policy_state.as_str(),
		"terminalization_state": run.terminalization_state.as_str(),
		"lane_control_next_action": run.lane_control_next_action.as_str(),
		"lane_control_conditions": &run.lane_control_conditions,
		"thread_status": run.thread_status.as_deref(),
		"process_id": run.process_id,
		"process_alive": run.process_alive,
		"process_liveness_reason": run.process_liveness_reason.as_deref(),
		"branch": run.branch_name.as_deref(),
		"worktree_path": run.worktree_path.as_deref(),
		"last_event_type": run.last_event_type.as_deref(),
		"last_event_at": run.last_event_at.as_deref(),
		"event_count": run.event_count,
		"control_capability": control_capability,
	})
}

pub(super) fn absolute_lane_worktree_path(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
) -> Result<Option<PathBuf>> {
	if let Some(mapping) = state_store.worktree_for_issue(&run.issue_id)? {
		return Ok(Some(mapping.worktree_path().to_path_buf()));
	}

	let Some(worktree_path) = run.worktree_path.as_deref() else {
		return Ok(None);
	};
	let worktree_path = Path::new(worktree_path);

	Ok(Some(if worktree_path.is_absolute() {
		worktree_path.to_path_buf()
	} else {
		project.repo_root().join(worktree_path)
	}))
}
