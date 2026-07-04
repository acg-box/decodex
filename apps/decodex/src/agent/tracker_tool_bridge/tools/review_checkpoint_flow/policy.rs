use crate::agent::tracker_tool_bridge::{
	NormalizedReviewCheckpointPayload, REVIEW_POLICY_CONVERGENCE_BUDGET, ReviewHandoffContext,
	ReviewPolicyPhase, ReviewPolicyStatus, TrackerToolBridge,
	tools::review_checkpoint::{self, ReviewFindingPolicyUpdate},
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint_flow) fn review_checkpoint_finding_policy_update(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<ReviewFindingPolicyUpdate, String> {
		let previous_state = self
			.review_policy_artifact_for_head(review_context, review_policy_phase, head_sha)
			.map_err(|error| error.to_string())?;
		let previous_finding_policy = previous_state
			.as_ref()
			.and_then(|previous_state| {
				review_checkpoint::review_finding_policy_from_previous_state(
					previous_state,
					review_policy_phase,
				)
			})
			.unwrap_or_default();
		let previous_nonclean_rounds = previous_state
			.as_ref()
			.filter(|previous_state| previous_state.phase == review_policy_phase)
			.map_or(0, |previous_state| previous_state.nonclean_rounds);
		let prior_nonclean_rounds_present = self
			.state_store
			.map(|state_store| {
				state_store.has_nonclean_review_checkpoint_artifact(
					&review_context.service_id,
					&self.issue.id,
					review_policy_phase.as_str(),
				)
			})
			.transpose()
			.map_err(|error| error.to_string())?
			.unwrap_or(false);
		let previous_nonclean_rounds = if prior_nonclean_rounds_present {
			previous_nonclean_rounds.max(1)
		} else {
			previous_nonclean_rounds
		};
		let previous_threshold_exceeded = previous_state.as_ref().is_some_and(|previous_state| {
			previous_state.phase == review_policy_phase
				&& previous_state.status == ReviewPolicyStatus::Findings
				&& previous_state.nonclean_rounds >= REVIEW_POLICY_CONVERGENCE_BUDGET
		});

		if review_policy_status == ReviewPolicyStatus::Findings
			&& (previous_finding_policy.stop_fingerprint.is_some() || previous_threshold_exceeded)
		{
			return Err(format!(
				"Review churn threshold already exceeded for issue `{}`; do not record another findings checkpoint. Route through architecture recovery or human attention before making further repair mutations.",
				self.issue.identifier
			));
		}

		Ok(review_checkpoint::review_finding_policy_update(
			previous_finding_policy,
			previous_nonclean_rounds,
			review_policy_phase,
			review_policy_status,
			head_sha,
			checkpoint_payload,
		))
	}
}
