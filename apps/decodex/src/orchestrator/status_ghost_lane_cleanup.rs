//! Missing-issue ghost-lane cleanup status projection.

use std::{
	collections::{BTreeSet, HashSet},
	path::Path,
	slice,
};

use time::OffsetDateTime;

use crate::{
	commit_message,
	config::ServiceConfig,
	state::StateStore,
	tracker::{self, IssueTracker, TrackerIssue, records::LinearExecutionEventRecord},
	workflow::WorkflowDocument,
};

use super::{
	GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING, GHOST_LANE_NEXT_ACTION, GHOST_LANE_OWNERSHIP_STATE,
	GHOST_LANE_POLICY_STATE, GHOST_LANE_TERMINAL_STATUS, OperatorRunStatus, OperatorStatusSnapshot,
	OperatorWorktreeStatus, operator_issue_attention_key, operator_project_display_name,
	operator_run_has_recent_app_server_execution, operator_run_tracker_issue_identifier_selector,
	recoverable_worktree_identifiers, status_ghost_lane_evidence,
	status_run_projection::{issue_identifier_in_text, operator_run_status},
};

pub(crate) fn ghost_lane_cleanup_status_blockers<T>(
	tracker: &T,
	project: &ServiceConfig,
	_workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
) -> crate::prelude::Result<Vec<String>>
where
	T: IssueTracker,
{
	let Some(mut run) = ghost_lane_cleanup_status_run(project, state_store, issue_id, run_id)?
	else {
		return Ok(vec![String::from("status_current_lane_missing")]);
	};

	if let Some(issue) = ghost_lane_tracker_issue(tracker, &run)? {
		return Ok(vec![
			String::from("tracker_issue_present"),
			format!("issue_state:{}", issue.state.name),
		]);
	}

	append_lane_control_condition(&mut run, GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING);
	apply_missing_issue_ghost_lane_status_projection(project, state_store, &mut run)?;

	if missing_issue_ghost_lane_status_allows_cleanup(&run)
		|| missing_issue_ghost_lane_status_is_cleanup_complete(&run)
	{
		return Ok(Vec::new());
	}

	let mut blockers = vec![
		format!("ownership_state:{}", run.ownership_state),
		format!("policy_state:{}", run.policy_state),
		format!("lane_control_next_action:{}", run.lane_control_next_action),
	];

	blockers.extend(run.lane_control_conditions.iter().cloned());
	blockers.sort();
	blockers.dedup();

	Ok(blockers)
}

pub(super) fn mark_operator_run_tracker_issue_missing(
	snapshot: &mut OperatorStatusSnapshot,
	run_id: &str,
	issue_id: &str,
	selector: &str,
) {
	for run in snapshot.current_lanes.iter_mut().chain(snapshot.recent_runs.iter_mut()) {
		if run.run_id == run_id || run.issue_id == issue_id {
			if run.issue_identifier.is_none() {
				run.issue_identifier = Some(selector.to_owned());
			}

			append_lane_control_condition(run, GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING);
		}
	}
}

pub(super) fn apply_missing_issue_ghost_lane_projection(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<()> {
	let current_worktree_keys = snapshot
		.worktrees
		.iter()
		.filter(|worktree| operator_worktree_status_path_exists(project, worktree))
		.map(|worktree| {
			operator_issue_attention_key(&worktree.issue_id, worktree.issue_identifier.as_deref())
		})
		.collect::<BTreeSet<_>>();
	let mut cleanup_complete_run_ids = HashSet::new();

	for run in &mut snapshot.current_lanes {
		if !run
			.lane_control_conditions
			.iter()
			.any(|condition| condition == GHOST_LANE_CONDITION_TRACKER_ISSUE_MISSING)
		{
			continue;
		}

		let (cleanup_safe, conditions) = missing_issue_ghost_lane_local_conditions(
			project,
			state_store,
			run,
			&current_worktree_keys,
		)?;

		for condition in conditions {
			append_lane_control_condition(run, &condition);
		}

		if cleanup_safe && missing_issue_ghost_lane_cleanup_audit_present(run) {
			apply_missing_issue_ghost_lane_cleanup_complete_run_projection(run);

			cleanup_complete_run_ids.insert(run.run_id.clone());
		} else if cleanup_safe {
			run.ownership_state = String::from(GHOST_LANE_OWNERSHIP_STATE);
			run.policy_state = String::from(GHOST_LANE_POLICY_STATE);
			run.lane_control_next_action = String::from(GHOST_LANE_NEXT_ACTION);
			run.needs_attention = true;
		} else {
			run.ownership_state = String::from("retained_attention");
			run.policy_state = String::from("runtime_recovery_blocked");
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
			append_lane_control_condition(run, "ghost_lane_cleanup_audit_present");
			apply_missing_issue_ghost_lane_cleanup_complete_run_projection(run);
		}
	}

	snapshot.current_lanes.retain(|run| !missing_issue_ghost_lane_status_is_cleanup_complete(run));

	Ok(())
}

fn ghost_lane_cleanup_status_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
) -> crate::prelude::Result<Option<OperatorRunStatus>> {
	let (leased_runs, _) = state_store.list_project_runs_read_only(project.service_id(), 0)?;
	let Some(run) =
		leased_runs.into_iter().find(|run| run.issue_id() == issue_id && run.run_id() == run_id)
	else {
		return Ok(None);
	};
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;
	let project_display_name = operator_project_display_name(project);
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	Ok(Some(operator_run_status(
		project,
		&loop_evidence,
		&project_display_name,
		run,
		now_unix_epoch,
	)?))
}

fn ghost_lane_tracker_issue<T>(
	tracker: &T,
	run: &OperatorRunStatus,
) -> crate::prelude::Result<Option<TrackerIssue>>
where
	T: IssueTracker,
{
	if !run.issue_id.trim().is_empty() && !run.issue_id.eq_ignore_ascii_case("unknown") {
		match tracker.refresh_issues(slice::from_ref(&run.issue_id)) {
			Ok(issues) =>
				if let Some(issue) = issues.into_iter().next() {
					return Ok(Some(issue));
				},
			Err(error)
				if tracker::issue_lookup_missing_error_for_candidate(&error, &run.issue_id) => {},
			Err(error) => return Err(error),
		}
	}

	let Some(selector) = operator_run_tracker_issue_identifier_selector(run) else {
		return Ok(None);
	};

	match tracker.get_issue_by_identifier(&selector) {
		Ok(issue) => Ok(issue),
		Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) =>
			Ok(None),
		Err(error) => Err(error),
	}
}

fn apply_missing_issue_ghost_lane_status_projection(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &mut OperatorRunStatus,
) -> crate::prelude::Result<()> {
	let current_worktree_keys = ghost_lane_current_worktree_keys(project, state_store)?;
	let (cleanup_safe, conditions) = missing_issue_ghost_lane_local_conditions(
		project,
		state_store,
		run,
		&current_worktree_keys,
	)?;

	for condition in conditions {
		append_lane_control_condition(run, &condition);
	}

	if cleanup_safe && missing_issue_ghost_lane_cleanup_audit_present(run) {
		apply_missing_issue_ghost_lane_cleanup_complete_run_projection(run);
	} else if cleanup_safe {
		run.ownership_state = String::from(GHOST_LANE_OWNERSHIP_STATE);
		run.policy_state = String::from(GHOST_LANE_POLICY_STATE);
		run.lane_control_next_action = String::from(GHOST_LANE_NEXT_ACTION);
	} else {
		run.ownership_state = String::from("retained_attention");
		run.policy_state = String::from("runtime_recovery_blocked");
		run.lane_control_next_action =
			String::from("inspect_missing_issue_runtime_recovery_blockers");
	}

	Ok(())
}

fn missing_issue_ghost_lane_status_allows_cleanup(run: &OperatorRunStatus) -> bool {
	run.ownership_state == GHOST_LANE_OWNERSHIP_STATE
		&& run.policy_state == GHOST_LANE_POLICY_STATE
		&& run.lane_control_next_action == GHOST_LANE_NEXT_ACTION
}

fn missing_issue_ghost_lane_status_is_cleanup_complete(run: &OperatorRunStatus) -> bool {
	run.ownership_state == "closed"
		&& run.policy_state == "allowed"
		&& run.lane_control_next_action == "no_action"
		&& missing_issue_ghost_lane_cleanup_audit_present(run)
}

fn missing_issue_ghost_lane_cleanup_audit_present(run: &OperatorRunStatus) -> bool {
	run.lane_control_conditions
		.iter()
		.any(|condition| condition == "ghost_lane_cleanup_audit_present")
}

fn apply_missing_issue_ghost_lane_cleanup_complete_run_projection(run: &mut OperatorRunStatus) {
	run.status = String::from(GHOST_LANE_TERMINAL_STATUS);
	run.attempt_status = String::from(GHOST_LANE_TERMINAL_STATUS);
	run.status_projection_reason = None;
	run.ownership_state = String::from("closed");
	run.liveness_state = String::from("not_running");
	run.policy_state = String::from("allowed");
	run.terminalization_state = String::from("cleanup_complete");
	run.lane_control_next_action = String::from("no_action");
	run.phase = String::from("completed");
	run.run_phase = String::from("completed");
	run.wait_reason = None;
	run.current_operation = String::from("ghost_lane_cleanup_audit");
	run.control_capability = None;
	run.continuation_pending = false;
	run.run_lease = false;
	run.queue_lease_state = String::from("not_held");
	run.execution_liveness = String::from("not_running");
	run.has_fresh_execution = false;
	run.counts_as_running = false;
	run.needs_attention = false;
	run.suspected_stall = false;
	run.retry_kind = None;
	run.next_retry_at = None;

	if let Some(loop_status) = run.loop_status.as_mut() {
		loop_status.summary = String::from("missing-issue ghost cleanup audit recorded");
		loop_status.next_action = None;
		loop_status.review = None;
	}
}

fn ghost_lane_current_worktree_keys(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> crate::prelude::Result<BTreeSet<String>> {
	let mut keys = BTreeSet::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if !mapping.worktree_path().exists() {
			continue;
		}

		let issue_identifier = issue_identifier_in_text(mapping.branch_name())
			.or_else(|| issue_identifier_in_text(&mapping.worktree_path().display().to_string()));

		keys.insert(operator_issue_attention_key(mapping.issue_id(), issue_identifier.as_deref()));
	}
	for issue_identifier in recoverable_worktree_identifiers(project.worktree_root())? {
		if project.worktree_root().join(&issue_identifier).exists() {
			keys.insert(operator_issue_attention_key(&issue_identifier, Some(&issue_identifier)));
		}
	}

	Ok(keys)
}

fn operator_worktree_status_path_exists(
	project: &ServiceConfig,
	worktree: &OperatorWorktreeStatus,
) -> bool {
	let path = Path::new(&worktree.worktree_path);

	if path.is_absolute() { path.exists() } else { project.repo_root().join(path).exists() }
}

fn missing_issue_ghost_lane_local_conditions(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	current_worktree_keys: &BTreeSet<String>,
) -> crate::prelude::Result<(bool, Vec<String>)> {
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
	inspect_status_ghost_lane_review_lineage(
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
) -> crate::prelude::Result<()> {
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

	let selector = operator_run_tracker_issue_identifier_selector(run);

	for candidate in [selector.as_deref(), Some(run.issue_id.as_str())].into_iter().flatten() {
		if commit_message::looks_like_issue_identifier(candidate)
			&& project.worktree_root().join(candidate).exists()
		{
			retained_worktree_present = true;
		}
	}

	let run_issue_key =
		operator_issue_attention_key(&run.issue_id, run.issue_identifier.as_deref());

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
	if operator_run_has_recent_app_server_execution(run) {
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
) -> crate::prelude::Result<()> {
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

fn inspect_status_ghost_lane_review_lineage(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
	conditions: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> crate::prelude::Result<()> {
	if state_store.issue_has_review_lifecycle_record(project.service_id(), &run.issue_id)? {
		blockers.push(String::from("review_lifecycle_present"));

		return Ok(());
	}
	if status_run_has_review_policy_checkpoint(project, state_store, run)? {
		blockers.push(String::from("review_policy_checkpoint_present"));

		return Ok(());
	}

	let mut records =
		state_store.list_linear_execution_events(project.service_id(), &run.issue_id)?;

	if let Some(issue_identifier) = run
		.issue_identifier
		.as_deref()
		.filter(|identifier| !identifier.eq_ignore_ascii_case(&run.issue_id))
	{
		records.extend(
			state_store.list_linear_execution_events(project.service_id(), issue_identifier)?,
		);
	}

	if records.iter().any(operator_linear_execution_event_has_pr_or_review_lineage) {
		blockers.push(String::from("pr_or_review_lineage_present"));
	} else {
		conditions.push(String::from("review_lineage_missing"));
	}

	Ok(())
}

fn status_run_has_review_policy_checkpoint(
	project: &ServiceConfig,
	state_store: &StateStore,
	run: &OperatorRunStatus,
) -> crate::prelude::Result<bool> {
	for phase in ["handoff", "repair"] {
		if state_store
			.review_policy_checkpoint(
				project.service_id(),
				&run.issue_id,
				&run.run_id,
				run.attempt_number,
				phase,
			)?
			.is_some()
		{
			return Ok(true);
		}
	}

	Ok(false)
}

fn operator_linear_execution_event_has_pr_or_review_lineage(
	record: &LinearExecutionEventRecord,
) -> bool {
	record.pr_url.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_head_sha.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_base_ref.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| matches!(
			record.event_type.as_str(),
			"review_handoff"
				| "review_handoff_rebind"
				| "review_handoff_adopt"
				| "review_repair"
				| "landed" | "closeout"
				| "cleanup_complete"
		) || record.terminal_path.as_deref() == Some("review_handoff")
}

fn append_lane_control_condition(run: &mut OperatorRunStatus, condition: &str) {
	if !run.lane_control_conditions.iter().any(|value| value == condition) {
		run.lane_control_conditions.push(condition.to_owned());
	}
}
