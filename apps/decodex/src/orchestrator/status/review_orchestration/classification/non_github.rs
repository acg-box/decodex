use crate::{
	orchestrator::status::{
		self, EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, PostReviewLaneClassification,
		PostReviewLaneDecision, PostReviewLifecycleAction, PullRequestReviewState,
	},
	prelude::Result,
	state::ReviewLifecycleRecord,
};

pub(crate) fn apply_non_github_review_post_review_classification(
	classification: &mut PostReviewLaneClassification,
	review_state: &PullRequestReviewState,
	lifecycle_record: Option<&ReviewLifecycleRecord>,
	now_unix_epoch: i64,
) -> Result<()> {
	if let Some(lifecycle_record) = lifecycle_record {
		let action = PostReviewLifecycleAction::parse(lifecycle_record.next_action())?;

		if matches!(
			action,
			PostReviewLifecycleAction::PollLandingReadback
				| PostReviewLifecycleAction::RunCloseoutAdapter
		) {
			if let Some(auto_merge_enabled_at) = lifecycle_record.auto_merge_enabled_at_unix_epoch()
				&& now_unix_epoch - auto_merge_enabled_at
					> EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
			{
				*classification = status::blocked_post_review_lane_from_state(
					review_state,
					"non_github_review_merge_visibility_timeout",
				);
			} else {
				classification.reason = String::from("non_github_review_waiting_for_merge");
			}

			return Ok(());
		}
		if action == PostReviewLifecycleAction::RunReviewRepair {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason =
				if status::review_state_landing_requires_agent_fallback(review_state) {
					String::from("retained_landing_agent_fallback_required")
				} else {
					String::from("non_github_review_repair_required")
				};

			return Ok(());
		}
	}

	if status::review_state_clean_path_landing_gates_satisfied(review_state) {
		classification.decision = PostReviewLaneDecision::ReadyToLand;
		classification.reason = String::from("non_github_review_ready_to_land");
	} else if status::review_state_landing_requires_agent_fallback(review_state) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("retained_landing_agent_fallback_required");
	} else {
		classification.reason = String::from("non_github_review_waiting_gates");
	}

	Ok(())
}
