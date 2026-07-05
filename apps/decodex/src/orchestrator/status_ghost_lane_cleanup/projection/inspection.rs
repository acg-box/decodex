mod control;
mod live;
mod private;
mod worktree;

use std::collections::BTreeSet;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		OperatorRunStatus,
		status_ghost_lane_cleanup::projection::{inspection, lineage},
		status_ghost_lane_evidence,
	},
	prelude::Result,
	state::StateStore,
};

pub(super) fn missing_issue_ghost_lane_local_conditions(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	current_worktree_keys: &BTreeSet<String>,
) -> Result<(bool, Vec<String>)> {
	let mut conditions = Vec::new();
	let mut blockers = Vec::new();

	if !run.run_lease {
		blockers.push(String::from("run_lease_missing"));
	}

	let mcp_test_fixture =
		status_ghost_lane_evidence::mcp_test_fixture_control_evidence(project, state_store, run)?;

	inspection::worktree::inspect_status_ghost_lane_worktree(
		project,
		state_store,
		run,
		current_worktree_keys,
		&mut conditions,
		&mut blockers,
	)?;
	inspection::control::inspect_status_ghost_lane_control_channel(
		run,
		mcp_test_fixture,
		&mut conditions,
		&mut blockers,
	);
	inspection::live::inspect_status_ghost_lane_live_evidence(
		run,
		mcp_test_fixture,
		&mut conditions,
		&mut blockers,
	);
	inspection::private::inspect_status_ghost_lane_private_evidence(
		project,
		state_store,
		run,
		mcp_test_fixture,
		&mut conditions,
		&mut blockers,
	)?;
	lineage::inspect_status_ghost_lane_review_lineage(
		project,
		state_store,
		run,
		&mut conditions,
		&mut blockers,
	)?;

	conditions.extend(blockers.iter().cloned());

	Ok((blockers.is_empty(), conditions))
}
