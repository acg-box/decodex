use std::collections::HashSet;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, OperatorHistoryLaneStatus, OperatorPostReviewLaneStatus, OperatorQueuedIssueStatus,
		OperatorRunStatus, OperatorStatusSnapshot, OperatorWorktreeStatus,
		kernel::state::{OwnershipState, PolicyState},
		status_run_projection::{self},
	},
	state::RUN_OPERATION_WAITING_EXTERNAL,
};

pub(super) fn refresh_operator_project_summary(
	snapshot: &mut OperatorStatusSnapshot,
	completed_state: Option<&str>,
) {
	let current_lane_count = snapshot.current_lanes.len();
	let running_lane_count =
		snapshot.current_lanes.iter().filter(|run| operator_run_counts_as_running(run)).count();
	let queued_candidate_count = snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| queued_candidate_counts_as_waiting_intake(candidate))
		.count();
	let post_review_lane_count =
		snapshot.post_review_lanes.iter().filter(|lane| !lane.shadowed_by_current_lane).count();
	let retained_worktree_count = orchestrator::rendered_recovery_worktrees(snapshot).len();
	let waiting_lane_count = project_waiting_lane_count(snapshot);
	let attention_count = project_attention_count(snapshot, completed_state);
	let cleanup_blocked_count = project_cleanup_blocked_count(snapshot);
	let cleanup_pending_count = project_cleanup_pending_count(snapshot);
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

pub(super) fn operator_run_counts_as_waiting(run: &OperatorRunStatus) -> bool {
	run.phase == "retry_backoff" || run.phase == "waiting_continuation" || run.wait_reason.is_some()
}

pub(super) fn queued_candidate_counts_as_waiting_intake(
	candidate: &OperatorQueuedIssueStatus,
) -> bool {
	!matches!(candidate.classification.as_str(), "claimed" | "closed")
}

pub(super) fn project_attention_count(
	snapshot: &OperatorStatusSnapshot,
	completed_state: Option<&str>,
) -> usize {
	let mut attention_keys = HashSet::new();

	for run in snapshot.current_lanes.iter().filter(|run| operator_run_counts_as_attention(run)) {
		attention_keys.insert(status_run_projection::operator_run_group_key(run));
	}
	for candidate in snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| queued_candidate_counts_as_attention(candidate))
	{
		attention_keys.insert(operator_issue_attention_key(
			&candidate.issue_id,
			Some(&candidate.issue_identifier),
		));
	}
	for lane in
		snapshot.post_review_lanes.iter().filter(|lane| post_review_lane_counts_as_attention(lane))
	{
		attention_keys
			.insert(operator_issue_attention_key(&lane.issue_id, Some(&lane.issue_identifier)));
	}
	for lane in snapshot
		.history_lanes
		.iter()
		.filter(|lane| history_lane_has_current_attention(snapshot, lane, completed_state))
	{
		attention_keys.insert(orchestrator::history_lane_group_key(lane));
	}

	attention_keys.len()
}

pub(super) fn project_history_only_attention_count(snapshot: &OperatorStatusSnapshot) -> usize {
	snapshot
		.history_lanes
		.iter()
		.filter(|lane| {
			orchestrator::history_ledger_outcome_requires_attention(&lane.ledger_outcome)
				&& !history_lane_has_current_attention_signal(snapshot, lane)
		})
		.count()
}

pub(super) fn operator_issue_attention_key(
	issue_id: &str,
	issue_identifier: Option<&str>,
) -> String {
	let issue_id = issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	if let Some(issue_identifier) = issue_identifier
		.map(str::trim)
		.filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("unknown"))
	{
		return issue_identifier.to_ascii_uppercase();
	}

	String::from("UNKNOWN")
}

pub(super) fn hydrate_post_review_lane_current_lane_shadowing(
	snapshot: &mut OperatorStatusSnapshot,
) {
	let current_lane_issue_keys = snapshot
		.current_lanes
		.iter()
		.filter(|run| run.counts_as_running)
		.map(|run| status_run_projection::operator_run_group_key(run).to_ascii_uppercase())
		.collect::<HashSet<_>>();

	for lane in &mut snapshot.post_review_lanes {
		lane.shadowed_by_current_lane = current_lane_issue_keys
			.contains(&operator_issue_attention_key(&lane.issue_id, Some(&lane.issue_identifier)));
	}
}

pub(super) fn operator_run_counts_as_current_lane(run: &OperatorRunStatus) -> bool {
	status_run_projection::operator_run_lane_control_readback(run).counts_as_current_lane
}

pub(super) fn operator_run_has_live_execution(run: &OperatorRunStatus) -> bool {
	status_run_projection::operator_run_lane_control_readback(run).has_live_execution
}

pub(super) fn operator_run_counts_as_running(run: &OperatorRunStatus) -> bool {
	if !run.ownership_state.is_empty() {
		return run.counts_as_running;
	}

	status_run_projection::operator_run_lane_control_readback(run).counts_as_running
}

pub(super) fn operator_run_counts_as_attention(run: &OperatorRunStatus) -> bool {
	let ownership = OwnershipState::from_str(&run.ownership_state);
	let policy = PolicyState::from_str(&run.policy_state);

	if !run.ownership_state.is_empty() {
		return run.needs_attention
			|| ownership == Some(OwnershipState::RetainedAttention)
			|| policy_requires_attention(policy);
	}

	status_run_projection::operator_run_lane_control_readback(run).counts_as_attention
}

pub(super) fn operator_run_has_recent_app_server_execution(run: &OperatorRunStatus) -> bool {
	matches!(run.thread_status.as_deref(), Some("active"))
		|| !run.thread_active_flags.is_empty()
		|| run.protocol_idle_for_seconds.is_some_and(|idle_for| {
			u64::try_from(idle_for)
				.is_ok_and(|idle_for| idle_for < RUN_LEASE_IDLE_TIMEOUT.as_secs())
		})
}

pub(super) fn operator_run_has_stale_execution_without_known_process(
	run: &OperatorRunStatus,
) -> bool {
	matches!(run.status.as_str(), "starting" | "running")
		&& run.phase == "executing"
		&& run.wait_reason.is_none()
		&& run.process_alive != Some(true)
		&& !run.has_fresh_execution
		&& [run.idle_for_seconds, run.protocol_idle_for_seconds].iter().any(|idle_for| {
			idle_for.is_some_and(|idle_for| {
				u64::try_from(idle_for)
					.is_ok_and(|idle_for| idle_for >= RUN_LEASE_IDLE_TIMEOUT.as_secs())
			})
		})
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
	if operator_run_counts_as_attention(run) {
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

fn queued_candidate_counts_as_attention(candidate: &OperatorQueuedIssueStatus) -> bool {
	candidate.classification == "blocked" || candidate.attention.is_some()
}

fn post_review_lane_counts_as_attention(lane: &OperatorPostReviewLaneStatus) -> bool {
	if lane.shadowed_by_current_lane {
		return false;
	}

	matches!(lane.classification.as_str(), "blocked" | "needs_review_repair" | "closeout_blocked")
}

fn history_lane_has_current_attention(
	snapshot: &OperatorStatusSnapshot,
	lane: &OperatorHistoryLaneStatus,
	completed_state: Option<&str>,
) -> bool {
	if !orchestrator::history_ledger_outcome_requires_attention(&lane.ledger_outcome) {
		return false;
	}

	history_lane_has_current_attention_signal(snapshot, lane)
		&& !history_lane_attention_is_resolved_tracker_echo(snapshot, lane, completed_state)
}

fn history_lane_has_current_attention_signal(
	snapshot: &OperatorStatusSnapshot,
	lane: &OperatorHistoryLaneStatus,
) -> bool {
	if lane.needs_attention_label_present == Some(true) {
		return true;
	}

	let issue_key = orchestrator::history_lane_group_key(lane);
	let has_non_attention_post_review_owner =
		history_lane_has_current_non_attention_post_review_owner(snapshot, &issue_key);

	if lane.active_label_present == Some(true) && !has_non_attention_post_review_owner {
		return true;
	}

	snapshot.worktrees.iter().any(|worktree| {
		!has_non_attention_post_review_owner
			&& operator_issue_attention_key(
				&worktree.issue_id,
				worktree.issue_identifier.as_deref(),
			) == issue_key
	}) || snapshot.post_review_lanes.iter().any(|post_review_lane| {
		post_review_lane_counts_as_attention(post_review_lane)
			&& operator_issue_attention_key(
				&post_review_lane.issue_id,
				Some(&post_review_lane.issue_identifier),
			) == issue_key
	}) || snapshot.queued_candidates.iter().any(|candidate| {
		queued_candidate_counts_as_attention(candidate)
			&& operator_issue_attention_key(&candidate.issue_id, Some(&candidate.issue_identifier))
				== issue_key
	})
}

fn history_lane_has_current_non_attention_post_review_owner(
	snapshot: &OperatorStatusSnapshot,
	issue_key: &str,
) -> bool {
	snapshot.post_review_lanes.iter().any(|post_review_lane| {
		!post_review_lane_counts_as_attention(post_review_lane)
			&& operator_issue_attention_key(
				&post_review_lane.issue_id,
				Some(&post_review_lane.issue_identifier),
			) == issue_key
	})
}

fn history_lane_attention_is_resolved_tracker_echo(
	snapshot: &OperatorStatusSnapshot,
	lane: &OperatorHistoryLaneStatus,
	completed_state: Option<&str>,
) -> bool {
	let Some(completed_state) = completed_state else {
		return false;
	};

	if lane.issue_state.as_deref() != Some(completed_state) {
		return false;
	}
	if lane.active_label_present != Some(false) || lane.needs_attention_label_present != Some(false)
	{
		return false;
	}

	let issue_key = orchestrator::history_lane_group_key(lane);

	!snapshot.worktrees.iter().any(|worktree| {
		operator_issue_attention_key(&worktree.issue_id, worktree.issue_identifier.as_deref())
			== issue_key
	}) && !snapshot.post_review_lanes.iter().any(|post_review_lane| {
		operator_issue_attention_key(
			&post_review_lane.issue_id,
			Some(&post_review_lane.issue_identifier),
		) == issue_key
	}) && !snapshot.queued_candidates.iter().any(|candidate| {
		if candidate.classification == "closed" && candidate.attention.is_none() {
			return false;
		}

		operator_issue_attention_key(&candidate.issue_id, Some(&candidate.issue_identifier))
			== issue_key
	})
}

fn project_cleanup_blocked_count(snapshot: &OperatorStatusSnapshot) -> usize {
	let mut cleanup_keys = HashSet::new();

	for lane in snapshot
		.post_review_lanes
		.iter()
		.filter(|lane| !lane.shadowed_by_current_lane && lane.classification == "cleanup_blocked")
	{
		cleanup_keys.insert(post_review_lane_cleanup_key(lane));
	}
	for worktree in snapshot.worktrees.iter().filter(|worktree| {
		worktree.hygiene.as_ref().is_some_and(|hygiene| {
			hygiene.dirty || hygiene.classification == "merged_dirty_worktree"
		})
	}) {
		cleanup_keys.insert(worktree_cleanup_key(worktree));
	}

	cleanup_keys.len()
}

fn project_cleanup_pending_count(snapshot: &OperatorStatusSnapshot) -> usize {
	snapshot
		.worktrees
		.iter()
		.filter(|worktree| {
			worktree.hygiene.as_ref().is_some_and(|hygiene| {
				!hygiene.dirty && hygiene.classification == "merged_worktree_cleanup_pending"
			})
		})
		.map(worktree_cleanup_key)
		.collect::<HashSet<_>>()
		.len()
}

fn post_review_lane_cleanup_key(lane: &OperatorPostReviewLaneStatus) -> String {
	if lane.issue_identifier.is_empty() {
		return lane.issue_id.clone();
	}

	lane.issue_identifier.clone()
}

fn worktree_cleanup_key(worktree: &OperatorWorktreeStatus) -> String {
	worktree.issue_identifier.clone().unwrap_or_else(|| worktree.issue_id.clone())
}

fn policy_requires_attention(policy: Option<PolicyState>) -> bool {
	matches!(
		policy,
		Some(
			PolicyState::ReviewChurnExceeded
				| PolicyState::ContinuationRecoveryChurnExceeded
				| PolicyState::AuthorityBoundaryRequired
				| PolicyState::HumanAttentionRequired
		)
	)
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
