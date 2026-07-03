use std::{collections::BTreeSet, path::Path};

use crate::{
	commit_message,
	config::ServiceConfig,
	orchestrator::{
		self, OperatorRunStatus, status_ghost_lane_cleanup::projection::lineage,
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

	inspect_status_ghost_lane_worktree(
		project,
		state_store,
		run,
		current_worktree_keys,
		&mut conditions,
		&mut blockers,
	)?;
	inspect_status_ghost_lane_control_channel(
		run,
		mcp_test_fixture,
		&mut conditions,
		&mut blockers,
	);
	inspect_status_ghost_lane_live_evidence(run, mcp_test_fixture, &mut conditions, &mut blockers);
	inspect_status_ghost_lane_private_evidence(
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

fn inspect_status_ghost_lane_worktree(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	current_worktree_keys: &BTreeSet<String>,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let mut retained_worktree_present = false;
	let mut mapping_checked = false;

	if let Some(worktree_path) = run.worktree_path.as_ref() {
		mapping_checked = true;

		if project.repo_root().join(worktree_path).exists() {
			retained_worktree_present = true;
		} else {
			conditions.push(String::from("worktree_mapping_path_missing"));
		}
	}
	if let Some(mapping) = state_store.worktree_for_issue(&run.issue_id)? {
		mapping_checked = true;

		if mapping.worktree_path().exists() {
			retained_worktree_present = true;
		} else {
			conditions.push(String::from("worktree_mapping_path_missing"));
		}
	}

	let selector = orchestrator::operator_run_tracker_issue_identifier_selector(run);

	for candidate in [selector.as_deref(), Some(run.issue_id.as_str())].into_iter().flatten() {
		if commit_message::looks_like_issue_identifier(candidate)
			&& project.worktree_root().join(candidate).exists()
		{
			retained_worktree_present = true;
		}
	}

	let run_issue_key =
		orchestrator::operator_issue_attention_key(&run.issue_id, run.issue_identifier.as_deref());

	if current_worktree_keys.contains(&run_issue_key) {
		retained_worktree_present = true;
	}
	if retained_worktree_present {
		blockers.push(String::from("retained_worktree_present"));
	} else {
		if !mapping_checked {
			conditions.push(String::from("worktree_mapping_missing"));
		}

		conditions.push(String::from("worktree_missing"));
	}

	Ok(())
}

fn inspect_status_ghost_lane_control_channel(
	run: &OperatorRunStatus,
	mcp_test_fixture: bool,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let Some(control_capability) = run.control_capability.as_ref() else {
		conditions.push(String::from("control_channel_missing"));

		return;
	};

	if Path::new(&control_capability.channel_path).exists() {
		conditions.push(String::from("control_channel_file_present"));
		blockers.push(String::from("control_channel_present"));
	} else {
		conditions.push(String::from("control_channel_file_missing"));

		if mcp_test_fixture {
			conditions.push(String::from("mcp_test_fixture_control_channel_row_present"));
		} else {
			blockers.push(String::from("control_channel_present"));
		}
	}
}

fn inspect_status_ghost_lane_live_evidence(
	run: &OperatorRunStatus,
	mcp_test_fixture: bool,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let mut live_blockers = Vec::new();

	if run.process_alive == Some(true) {
		live_blockers.push(String::from("process_alive"));
	}
	if matches!(run.thread_status.as_deref(), Some("active")) || !run.thread_active_flags.is_empty()
	{
		live_blockers.push(String::from("thread_active"));
	}
	if orchestrator::operator_run_has_recent_app_server_execution(run) {
		live_blockers.push(String::from("protocol_recent"));
	}
	if run.event_count > 0 || run.last_event_type.is_some() || run.last_event_at.is_some() {
		live_blockers.push(String::from("protocol_event_evidence_present"));
	}
	if run.child_agent_activity.is_some() {
		live_blockers.push(String::from("child_agent_activity_present"));
	}
	if run.protocol_activity.is_some() {
		live_blockers.push(String::from("protocol_activity_present"));
	}
	if run.thread_id.is_some() || run.turn_id.is_some() {
		live_blockers.push(String::from("thread_reference_present"));
	}
	if live_blockers.is_empty() {
		conditions.push(String::from("no_live_execution_evidence"));

		return;
	}
	if mcp_test_fixture
		&& live_blockers.iter().all(|blocker| {
			status_ghost_lane_evidence::mcp_test_fixture_allowed_live_blocker(blocker)
		}) {
		conditions.push(String::from("mcp_test_fixture_protocol_or_thread_evidence_present"));

		return;
	}

	blockers.extend(live_blockers);
}

fn inspect_status_ghost_lane_private_evidence(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	mcp_test_fixture: bool,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&run.issue_id,
		&run.run_id,
		run.attempt_number,
	)?;

	if events.is_empty() {
		conditions.push(String::from("private_evidence_missing"));
	} else if mcp_test_fixture {
		conditions.push(String::from("mcp_test_fixture_private_control_evidence_present"));

		if events.iter().any(status_ghost_lane_evidence::private_event_is_cleanup_audit) {
			conditions.push(String::from("ghost_lane_cleanup_audit_present"));
		}
	} else if status_ghost_lane_evidence::private_events_are_cleanup_audit_evidence(&events) {
		conditions.push(String::from("ghost_lane_cleanup_audit_present"));
	} else {
		blockers.push(String::from("private_evidence_present"));
	}

	Ok(())
}
