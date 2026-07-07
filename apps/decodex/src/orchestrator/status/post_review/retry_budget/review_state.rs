use crate::orchestrator::status::post_review::{
	PostReviewLaneSnapshot, PullRequestReviewState, PullRequestReviewStateInspector, retry_budget,
};

pub(crate) fn retry_budget_exhausted_merged_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> Option<PullRequestReviewState>
where
	I: PullRequestReviewStateInspector,
{
	let lifecycle_record = snapshot.lifecycle_record.as_ref()?;

	if !retry_budget::worktree_has_no_tracked_changes(snapshot.worktree.worktree_path()) {
		return None;
	}

	let review_state = review_state_inspector
		.inspect_review_state(snapshot.worktree.worktree_path(), lifecycle_record.pr_url())
		.ok()?;

	(review_state.state == "MERGED").then_some(review_state)
}
