use crate::agent::tracker_tool_bridge::{
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, NormalizedReviewCheckpointPayload, ReviewHandoffContext,
	ReviewPolicyPhase, ReviewPolicyStatus, TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint_flow) fn review_checkpoint_details_json(
		&self,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<String, String> {
		serde_json::to_string(checkpoint_payload).map_err(|error| {
			format!(
				"Failed to serialize the structured review checkpoint for issue `{}`: {error}",
				self.issue.identifier
			)
		})
	}

	pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint_flow) fn append_private_review_checkpoint(
		&self,
		review_context: &ReviewHandoffContext,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
	) -> Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{ISSUE_REVIEW_CHECKPOINT_TOOL_NAME}` requires the Decodex runtime state store for issue `{}`.",
				self.issue.identifier
			)
		})?;
		let private_payload = serde_json::json!({
			"phase": review_policy_phase.as_str(),
			"status": review_policy_status.as_str(),
			"head_sha": head_sha,
			"nonclean_rounds": nonclean_rounds,
			"active_fingerprints": &checkpoint_payload.finding_policy.active_fingerprints,
			"stop_fingerprint": &checkpoint_payload.finding_policy.stop_fingerprint,
			"route_counts": &checkpoint_payload.finding_route_summary.route_counts,
			"route_next_action": &checkpoint_payload.finding_route_summary.next_action,
			"review_class": &checkpoint_payload.review_cost_control.review_class,
			"risk_class": &checkpoint_payload.review_cost_control.risk_class,
			"compact_eligible": checkpoint_payload.review_cost_control.compact_eligible,
			"review_fallback_reason": &checkpoint_payload.review_cost_control.fallback_reason,
			"review": checkpoint_payload,
		});

		state_store
			.append_private_execution_event(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
				"review_checkpoint",
				private_payload,
			)
			.map(|_| ())
			.map_err(|error| {
				format!(
					"Failed to persist the private review checkpoint for issue `{}`: {error}",
					self.issue.identifier
				)
			})
	}
}
