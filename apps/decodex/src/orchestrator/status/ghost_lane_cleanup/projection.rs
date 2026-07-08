mod current_worktrees;
mod inspection;
mod lineage;

use std::collections::HashSet;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING, GHOST_LANE_NEXT_ACTION,
		GHOST_LANE_OWNERSHIP_STATE, GHOST_LANE_POLICY_STATE, OperatorRunStatus,
		OperatorStatusSnapshot,
		kernel::state::{OwnershipState, PolicyState},
		status_ghost_lane_cleanup::conditions,
	},
	prelude::Result,
	state::StateStore,
};

pub(crate) fn apply_missing_issue_ghost_lane_projection(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<()> {
	let current_worktree_keys =
		current_worktrees::operator_snapshot_current_worktree_keys(project, snapshot);
	let mut cleanup_complete_run_ids = HashSet::new();

	for run in &mut snapshot.current_lanes {
		if !run
			.lane_control_conditions
			.iter()
			.any(|condition| condition == GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING)
		{
			continue;
		}

		let (cleanup_safe, conditions) = inspection::missing_issue_ghost_lane_local_conditions(
			project,
			state_store,
			run,
			&current_worktree_keys,
		)?;

		for condition in conditions {
			conditions::append_lane_control_condition(run, &condition);
		}

		if cleanup_safe && conditions::missing_issue_ghost_lane_cleanup_audit_present(run) {
			conditions::apply_missing_issue_cleanup_projection(run);

			cleanup_complete_run_ids.insert(run.run_id.clone());
		} else if cleanup_safe {
			run.ownership_state = String::from(GHOST_LANE_OWNERSHIP_STATE);
			run.policy_state = String::from(GHOST_LANE_POLICY_STATE);
			run.lane_control_next_action = String::from(GHOST_LANE_NEXT_ACTION);
			run.needs_attention = true;
		} else {
			run.ownership_state = String::from(OwnershipState::RetainedAttention.as_str());
			run.policy_state = String::from(PolicyState::RuntimeRecoveryBlocked.as_str());
			run.lane_control_next_action =
				String::from("inspect_missing_issue_runtime_recovery_blockers");
			run.needs_attention = true;
		}

		if let Some(loop_status) = run.loop_status.as_mut() {
			loop_status.next_action = Some(run.lane_control_next_action.clone());
		}

		run.counts_as_running = false;
	}
	for run in &mut snapshot.recent_runs {
		if cleanup_complete_run_ids.contains(&run.run_id) {
			conditions::append_lane_control_condition(run, "ghost_lane_cleanup_audit_present");
			conditions::apply_missing_issue_cleanup_projection(run);
		}
	}

	snapshot
		.current_lanes
		.retain(|run| !conditions::missing_issue_ghost_lane_status_is_cleanup_complete(run));

	Ok(())
}

pub(crate) fn apply_missing_issue_ghost_lane_status_projection(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &mut OperatorRunStatus,
) -> Result<()> {
	let current_worktree_keys =
		current_worktrees::ghost_lane_current_worktree_keys(project, state_store)?;
	let (cleanup_safe, conditions) = inspection::missing_issue_ghost_lane_local_conditions(
		project,
		state_store,
		run,
		&current_worktree_keys,
	)?;

	for condition in conditions {
		conditions::append_lane_control_condition(run, &condition);
	}

	if cleanup_safe && conditions::missing_issue_ghost_lane_cleanup_audit_present(run) {
		conditions::apply_missing_issue_cleanup_projection(run);
	} else if cleanup_safe {
		run.ownership_state = String::from(GHOST_LANE_OWNERSHIP_STATE);
		run.policy_state = String::from(GHOST_LANE_POLICY_STATE);
		run.lane_control_next_action = String::from(GHOST_LANE_NEXT_ACTION);
	} else {
		run.ownership_state = String::from(OwnershipState::RetainedAttention.as_str());
		run.policy_state = String::from(PolicyState::RuntimeRecoveryBlocked.as_str());
		run.lane_control_next_action =
			String::from("inspect_missing_issue_runtime_recovery_blockers");
	}

	Ok(())
}
