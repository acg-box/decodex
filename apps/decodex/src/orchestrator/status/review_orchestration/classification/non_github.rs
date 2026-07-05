use crate::{
	orchestrator::status::{
		self, EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, PostReviewLaneClassification,
		PostReviewLaneDecision, PullRequestReviewState, ReviewOrchestrationMarker,
		ReviewOrchestrationPhase,
	},
	prelude::{Result, eyre},
};

pub(crate) fn apply_non_github_review_post_review_classification(
	classification: &mut PostReviewLaneClassification,
	review_state: &PullRequestReviewState,
	orchestration_marker: Option<&ReviewOrchestrationMarker>,
	now_unix_epoch: i64,
) -> Result<()> {
	if let Some(orchestration_marker) = orchestration_marker {
		let phase =
			ReviewOrchestrationPhase::parse(orchestration_marker.phase()).map_err(|error| {
				eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
			})?;

		if phase == ReviewOrchestrationPhase::WaitingForMerge {
			if let Some(auto_merge_enabled_at) =
				orchestration_marker.auto_merge_enabled_at_unix_epoch()
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
		if phase == ReviewOrchestrationPhase::RepairRequired {
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
