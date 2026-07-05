use crate::orchestrator::status::{
	self, PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
	PullRequestReviewState, WorkflowDocument,
};

pub(crate) fn apply_pre_orchestration_post_review_classification(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
) -> bool {
	if review_state.state == "MERGED" {
		classification.decision = PostReviewLaneDecision::Continue;
		classification.reason = String::from("pull_request_merged_closeout_pending");

		return true;
	}
	if snapshot.issue.state.name == workflow.frontmatter().tracker().resolved_completed_state() {
		*classification = status::blocked_post_review_lane_from_state(
			review_state,
			"issue_completed_before_pull_request_merged",
		);

		return true;
	}
	if review_state.state != "OPEN" {
		*classification =
			status::blocked_post_review_lane_from_state(review_state, "pull_request_not_open");

		return true;
	}
	if review_state.is_draft {
		*classification =
			status::blocked_post_review_lane_from_state(review_state, "pull_request_is_draft");

		return true;
	}
	if review_state.unresolved_review_threads > 0 {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("unresolved_review_threads");

		return true;
	}
	if matches!(review_state.review_decision.as_deref(), Some("CHANGES_REQUESTED")) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("review_changes_requested");

		return true;
	}
	if status::failed_checks_require_repair(
		review_state.status_check_rollup_state.as_deref(),
		&review_state.merge_state_status,
	) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("required_checks_failed");

		return true;
	}

	if let Some(reason) = status::merge_state_requires_review_repair(
		&review_state.mergeable,
		&review_state.merge_state_status,
	) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from(reason);

		return true;
	}

	false
}
