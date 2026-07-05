use crate::{
	agent::tracker_tool_bridge::{
		REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewHandoffContext, ReviewPolicyPhase,
		ReviewPolicyState, ReviewPolicyStatus, ReviewPolicyStopReason, ReviewPolicyStopRequested,
		TrackerToolBridge, review::linear_events,
	},
	prelude::Result,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn review_policy_stop_requested(
		&self,
		review_context: &ReviewHandoffContext,
	) -> Result<Option<ReviewPolicyStopRequested>> {
		if !review_context.decodex_review_checkpoint_enabled() {
			return Ok(None);
		}

		let Some(current_phase) = ReviewPolicyPhase::for_mode(review_context.mode) else {
			return Ok(None);
		};
		let Some(state_store) = self.state_store else {
			return Ok(None);
		};

		if !state_store.has_nonclean_review_checkpoint_artifact(
			&review_context.service_id,
			&self.issue.id,
			current_phase.as_str(),
		)? {
			return Ok(None);
		}

		let Some(checkpoint) = self.review_policy_state_for_current_head(review_context)? else {
			return Ok(None);
		};

		Ok(self.review_policy_stop_from_checkpoint(review_context, checkpoint))
	}

	fn review_policy_stop_from_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		checkpoint: ReviewPolicyState,
	) -> Option<ReviewPolicyStopRequested> {
		let stop_reason = match checkpoint.status {
			ReviewPolicyStatus::Clean => return None,
			ReviewPolicyStatus::Findings
				if checkpoint.nonclean_rounds < REVIEW_POLICY_CONVERGENCE_BUDGET =>
			{
				return None;
			},
			ReviewPolicyStatus::Findings => ReviewPolicyStopReason::Exhausted,
			ReviewPolicyStatus::NeedsArchitectureReview =>
				ReviewPolicyStopReason::ArchitectureReviewRequired,
			ReviewPolicyStatus::Blocked => ReviewPolicyStopReason::Blocked,
		};

		Some(ReviewPolicyStopRequested {
			head_sha: checkpoint.head_sha,
			issue_identifier: self.issue.identifier.clone(),
			fingerprint: linear_events::review_policy_stop_fingerprint(&checkpoint.details_json),
			nonclean_rounds: Some(checkpoint.nonclean_rounds),
			reason: stop_reason,
			run_id: review_context.run_id.clone(),
		})
	}
}
