use std::collections::HashSet;

use crate::orchestrator::{
	OperatorPostReviewLaneStatus, OperatorStatusSnapshot, OperatorWorktreeStatus,
};

pub(super) fn project_cleanup_blocked_count(snapshot: &OperatorStatusSnapshot) -> usize {
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

pub(super) fn project_cleanup_pending_count(snapshot: &OperatorStatusSnapshot) -> usize {
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
