use std::collections::HashSet;

use crate::orchestrator::{
	self, OperatorHistoryLaneStatus, OperatorPostReviewLaneStatus, OperatorStatusSnapshot,
	status_run_projection::{self},
	status_summary::{queue, run_state},
};

pub(super) fn project_attention_count(
	snapshot: &OperatorStatusSnapshot,
	completed_state: Option<&str>,
) -> usize {
	let mut attention_keys = HashSet::new();

	for run in
		snapshot.current_lanes.iter().filter(|run| run_state::operator_run_counts_as_attention(run))
	{
		attention_keys.insert(status_run_projection::operator_run_group_key(run));
	}
	for candidate in snapshot
		.queued_candidates
		.iter()
		.filter(|candidate| queue::queued_candidate_counts_as_attention(candidate))
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
		queue::queued_candidate_counts_as_attention(candidate)
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
