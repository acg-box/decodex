mod policy;
mod prepare;
mod response;
mod writeback;

use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, NormalizedReviewCheckpointPayload, ReviewCheckpointArgs,
	ReviewPolicyPhase, ReviewPolicyStatus, TrackerToolBridge, tools::review_checkpoint,
};

struct ReviewCheckpointPayloadCounts {
	evidence: usize,
	accepted_findings: usize,
	rejected_findings: usize,
	finding_routes: usize,
	current_blockers: usize,
}

struct PreparedReviewCheckpoint {
	review_policy_phase: ReviewPolicyPhase,
	review_policy_status: ReviewPolicyStatus,
	head_sha: String,
	checkpoint_payload: NormalizedReviewCheckpointPayload,
	nonclean_rounds: i64,
}

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn handle_review_checkpoint(&self, arguments: Value) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewCheckpointArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.review_checkpoint` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_checkpoint` is unavailable for this run.",
			));
		};

		if !review_context.decodex_review_checkpoint_enabled() {
			return DynamicToolCallResponse::failure(format!(
				"`issue_review_checkpoint` is disabled because `[codex].review = \"{}\"` for this run.",
				review_context.review_level.as_str()
			));
		}

		let prepared = match self.prepare_review_checkpoint(parsed, review_context) {
			Ok(prepared) => prepared,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let details_json = match self.review_checkpoint_details_json(&prepared.checkpoint_payload) {
			Ok(details_json) => details_json,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.persist_review_policy_state(
			review_context,
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			&details_json,
		) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_private_review_checkpoint(
			review_context,
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			&prepared.checkpoint_payload,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		let message = self.review_checkpoint_success_message(
			prepared.review_policy_phase,
			prepared.review_policy_status,
			&prepared.head_sha,
			prepared.nonclean_rounds,
			ReviewCheckpointPayloadCounts {
				evidence: prepared.checkpoint_payload.evidence.len(),
				accepted_findings: prepared.checkpoint_payload.accepted_findings.len(),
				rejected_findings: prepared.checkpoint_payload.rejected_findings.len(),
				finding_routes: prepared.checkpoint_payload.finding_routes.len(),
				current_blockers: review_checkpoint::current_review_blocker_findings(
					&prepared.checkpoint_payload,
				)
				.count(),
			},
		);

		if let Some(response) = self.review_checkpoint_churn_stop_response(
			prepared.review_policy_status,
			prepared.nonclean_rounds,
			&prepared.checkpoint_payload,
			&message,
		) {
			return response;
		}

		DynamicToolCallResponse::success(message)
	}
}
