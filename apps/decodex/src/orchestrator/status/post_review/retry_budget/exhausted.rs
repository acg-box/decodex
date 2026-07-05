use crate::orchestrator::status::post_review::{
	self, PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
	PullRequestReviewStateInspector, ServiceConfig, WorkflowDocument, retry_budget,
};

pub(crate) fn retry_budget_exhausted_post_review_lane_classification<I>(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	mut classification: PostReviewLaneClassification,
) -> PostReviewLaneClassification
where
	I: PullRequestReviewStateInspector,
{
	if classification.pr_url.is_none() {
		classification.pr_url =
			snapshot.review_handoff.as_ref().map(|marker| marker.pr_url().to_owned());
	}
	if classification.pr_state.is_none()
		&& let Some(review_state) = retry_budget::retry_budget_exhausted_merged_review_state(
			snapshot,
			review_state_inspector,
		) {
		classification = post_review::initial_post_review_lane_classification(&review_state);

		post_review::apply_pre_orchestration_post_review_classification(
			snapshot,
			workflow,
			&review_state,
			&mut classification,
		);
	}
	if retry_budget::merged_closeout_pending_classification(&classification)
		&& retry_budget::worktree_has_no_tracked_changes(snapshot.worktree.worktree_path())
	{
		if snapshot.issue.state.name == workflow.frontmatter().tracker().resolved_completed_state()
			&& !retry_budget::worktree_has_no_tracked_changes(project.repo_root())
		{
			classification.decision = PostReviewLaneDecision::CleanupBlocked;
			classification.reason = String::from("default_branch_worktree_dirty");

			return post_review::finalize_post_review_lane_classification_with_retry_budget(
				snapshot,
				classification,
				true,
			);
		}

		return post_review::finalize_post_review_lane_classification(snapshot, classification);
	}
	if classification.pr_state.as_deref() == Some("MERGED")
		&& retry_budget::worktree_has_no_tracked_changes(snapshot.worktree.worktree_path())
	{
		classification.decision = if snapshot.issue.state.name
			== workflow.frontmatter().tracker().resolved_completed_state()
		{
			PostReviewLaneDecision::CleanupBlocked
		} else {
			PostReviewLaneDecision::CloseoutBlocked
		};
		classification.reason = String::from("retry_budget_exhausted");

		return post_review::finalize_post_review_lane_classification_with_retry_budget(
			snapshot,
			classification,
			true,
		);
	}

	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = String::from("retry_budget_exhausted");

	post_review::finalize_post_review_lane_classification_with_retry_budget(
		snapshot,
		classification,
		true,
	)
}

pub(crate) fn merged_closeout_pending_classification(
	classification: &PostReviewLaneClassification,
) -> bool {
	classification.decision == PostReviewLaneDecision::Continue
		&& classification.reason == "pull_request_merged_closeout_pending"
		&& classification.pr_state.as_deref() == Some("MERGED")
}
