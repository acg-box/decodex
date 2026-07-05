use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	PendingReviewCompletion, PullRequestDetails, ReviewHandoffContext, RunCompletionDisposition,
	TrackerToolBridge, review, tools::REVIEW_COMPLETION_INTENT_EVENT_TYPE,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn persist_terminal_review_handoff_marker(
		&self,
		review_context: &ReviewHandoffContext,
	) -> std::result::Result<(), String> {
		let pending_review_handoff = {
			let pending_review_handoff = self.pending_review_completion.borrow();
			let Some(PendingReviewCompletion::Handoff(pending_review_handoff)) =
				pending_review_handoff.as_ref()
			else {
				return Err(format!(
					"`issue_terminal_finalize` cannot persist review handoff lifecycle state for issue `{}` because no PR-backed review handoff is pending.",
					self.issue.identifier
				));
			};

			pending_review_handoff.clone()
		};
		let pull_request =
			self.validate_review_action_pr(review_context, &pending_review_handoff.pr_url)?;

		self.require_matching_review_completion_intent(
			review_context,
			RunCompletionDisposition::ReviewHandoff,
			&pull_request,
		)?;

		let handoff_marker =
			review::review_handoff_marker_from_pull_request(review_context, &pull_request);

		self.persist_review_handoff_marker_for_handoff(review_context, &handoff_marker).map_err(
			|error| {
				format!(
					"Failed to persist durable review handoff lifecycle marker for issue `{}`: {error}",
					self.issue.identifier
				)
			},
		)
	}

	fn require_matching_review_completion_intent(
		&self,
		review_context: &ReviewHandoffContext,
		path: RunCompletionDisposition,
		pull_request: &PullRequestDetails,
	) -> std::result::Result<(), String> {
		let state_store = self.state_store.ok_or_else(|| {
			format!(
				"`{}` requires the Decodex runtime state store for issue `{}`.",
				self.required_pr_completion_tool_name(),
				self.issue.identifier
			)
		})?;
		let events = state_store
			.list_private_execution_events(
				&review_context.service_id,
				&self.issue.id,
				&review_context.run_id,
				review_context.attempt_number,
			)
			.map_err(|error| {
				format!(
					"Failed to read private review completion intent for issue `{}`: {error}",
					self.issue.identifier
				)
			})?;
		let exact_intent_exists = events.iter().rev().any(|event| {
			let payload = event.payload();

			event.event_type() == REVIEW_COMPLETION_INTENT_EVENT_TYPE
				&& payload.get("path").and_then(Value::as_str) == Some(path.as_str())
				&& payload.get("mode").and_then(Value::as_str) == Some(review_context.mode.as_str())
				&& payload.get("branch").and_then(Value::as_str)
					== Some(review_context.branch_name.as_str())
				&& payload.get("worktree_path").and_then(Value::as_str)
					== Some(review_context.worktree_path.as_str())
				&& payload.get("pr_url").and_then(Value::as_str) == Some(pull_request.url.as_str())
				&& payload.get("pr_base_ref").and_then(Value::as_str)
					== Some(pull_request.base_ref_name.as_str())
				&& payload.get("pr_head_ref").and_then(Value::as_str)
					== Some(pull_request.head_ref_name.as_str())
				&& payload.get("pr_head_oid").and_then(Value::as_str)
					== Some(pull_request.head_ref_oid.as_str())
		});

		if exact_intent_exists {
			return Ok(());
		}

		Err(format!(
			"`issue_terminal_finalize` requires an exact private review completion intent for issue `{}` before writing local review handoff lifecycle state.",
			self.issue.identifier
		))
	}
}
