use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	self, DynamicToolCallResponse, ISSUE_REVIEW_HANDOFF_TOOL_NAME, PendingReviewAction,
	PendingReviewCompletion, ReviewExecutionMode, ReviewHandoffArgs, RunCompletionDisposition,
	TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools) fn handle_review_handoff(
		&self,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewHandoffArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.review_handoff` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` is unavailable for this run.",
			));
		};

		if review_context.mode != ReviewExecutionMode::Handoff {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` is unavailable for retained review-repair runs.",
			));
		}

		let pr_url = parsed.pr_url.trim();

		if pr_url.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` requires a non-empty `pr_url`.",
			));
		}

		let summary = tracker_tool_bridge::normalize_summary(&parsed.summary);

		if summary.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_handoff` requires a non-empty `summary`.",
			));
		}

		let pull_request = match self.validate_review_action_pr(review_context, pr_url) {
			Ok(pull_request) => pull_request,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.clear_review_policy_state_after_completion(
			review_context,
			ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_review_completion_intent(
			review_context,
			RunCompletionDisposition::ReviewHandoff,
			&pull_request,
			&summary,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		self.pending_review_completion.borrow_mut().replace(PendingReviewCompletion::Handoff(
			PendingReviewAction { pr_url: pull_request.url.clone(), summary },
		));

		DynamicToolCallResponse::success(format!(
			"Recorded review handoff for issue `{}` with PR `{}`. Decodex will apply the completion comment and move the issue to `{}` after service validation passes.",
			self.issue.identifier,
			pull_request.url,
			self.workflow.frontmatter().tracker().success_state()
		))
	}
}
