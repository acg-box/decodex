use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, NormalizedReviewCheckpointPayload, REVIEW_POLICY_CONVERGENCE_BUDGET,
	ReviewPolicyPhase, ReviewPolicyStatus, TrackerToolBridge,
	tools::review_checkpoint_flow::ReviewCheckpointPayloadCounts,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint_flow) fn review_checkpoint_churn_stop_response(
		&self,
		review_policy_status: ReviewPolicyStatus,
		nonclean_rounds: i64,
		checkpoint_payload: &NormalizedReviewCheckpointPayload,
		message: &str,
	) -> Option<DynamicToolCallResponse> {
		if review_policy_status != ReviewPolicyStatus::Findings
			|| nonclean_rounds < REVIEW_POLICY_CONVERGENCE_BUDGET
		{
			return None;
		}

		let fingerprint = checkpoint_payload
			.finding_policy
			.stop_fingerprint
			.as_ref()
			.map_or_else(String::new, |fingerprint| {
				format!(" Finding fingerprint `{fingerprint}` caused the stop.")
			});

		Some(DynamicToolCallResponse::failure(format!(
			"{message} Review churn threshold exceeded.{fingerprint} Stop the current repair strategy now and route through architecture recovery or human attention before making further repair mutations."
		)))
	}

	pub(in crate::agent::tracker_tool_bridge::tools::review_checkpoint_flow) fn review_checkpoint_success_message(
		&self,
		review_policy_phase: ReviewPolicyPhase,
		review_policy_status: ReviewPolicyStatus,
		head_sha: &str,
		nonclean_rounds: i64,
		counts: ReviewCheckpointPayloadCounts,
	) -> String {
		let evidence_suffix = format!(
			"{} evidence item(s), {} accepted finding(s), {} rejected finding(s), {} route(s), and {} current blocker(s) recorded",
			counts.evidence,
			counts.accepted_findings,
			counts.rejected_findings,
			counts.finding_routes,
			counts.current_blockers,
		);

		match review_policy_status {
			ReviewPolicyStatus::Clean => format!(
				"Recorded a clean `{}` review checkpoint for issue `{}` at HEAD `{head_sha}`; {evidence_suffix}.",
				review_policy_phase.as_str(),
				self.issue.identifier,
			),
			ReviewPolicyStatus::Findings => format!(
				"Recorded `{}` review findings for issue `{}` at HEAD `{head_sha}`; max unresolved finding repeat count now `{nonclean_rounds}`; {evidence_suffix}.",
				review_policy_phase.as_str(),
				self.issue.identifier,
			),
			ReviewPolicyStatus::NeedsArchitectureReview => format!(
				"Recorded `needs_architecture_review` for issue `{}` at HEAD `{head_sha}`; Decodex will require human architecture review if the turn ends on this checkpoint.",
				self.issue.identifier,
			),
			ReviewPolicyStatus::Blocked => format!(
				"Recorded `blocked` for issue `{}` at HEAD `{head_sha}`; Decodex will require human intervention if the turn ends on this checkpoint.",
				self.issue.identifier,
			),
		}
	}
}
