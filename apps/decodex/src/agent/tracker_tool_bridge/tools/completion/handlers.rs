use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	self, DynamicToolCallResponse, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, PendingReviewAction,
	PendingReviewCompletion, ReviewExecutionMode, ReviewHandoffArgs, RunCompletionDisposition,
	TerminalFinalizeArgs, TrackerToolBridge,
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

		if let Err(error) = self.require_clean_review_checkpoint(review_context) {
			return DynamicToolCallResponse::failure(error);
		}
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

	pub(in crate::agent::tracker_tool_bridge::tools) fn handle_review_repair_complete(
		&self,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewHandoffArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.review_repair_complete` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` is unavailable for this run.",
			));
		};

		if review_context.mode != ReviewExecutionMode::Repair {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` is unavailable before a retained in-review repair run starts.",
			));
		}

		let pr_url = parsed.pr_url.trim();

		if pr_url.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` requires a non-empty `pr_url`.",
			));
		}

		let summary = tracker_tool_bridge::normalize_summary(&parsed.summary);

		if summary.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_review_repair_complete` requires a non-empty `summary`.",
			));
		}

		let pull_request = match self.validate_review_action_pr(review_context, pr_url) {
			Ok(pull_request) => pull_request,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.require_clean_review_checkpoint(review_context) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.clear_review_policy_state_after_completion(
			review_context,
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME,
		) {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_review_completion_intent(
			review_context,
			RunCompletionDisposition::ReviewRepair,
			&pull_request,
			&summary,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		self.pending_review_completion.borrow_mut().replace(PendingReviewCompletion::Repair(
			PendingReviewAction { pr_url: pull_request.url.clone(), summary },
		));

		DynamicToolCallResponse::success(format!(
			"Recorded retained review repair completion for issue `{}` on PR `{}`. Decodex will persist the updated review lineage after service validation passes.",
			self.issue.identifier, pull_request.url
		))
	}

	pub(in crate::agent::tracker_tool_bridge::tools) fn handle_closeout_complete(
		&self,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ReviewHandoffArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.closeout_complete` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` is unavailable for this run.",
			));
		};

		if review_context.mode != ReviewExecutionMode::Closeout {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` is unavailable before a retained post-review closeout run starts.",
			));
		}

		let pr_url = parsed.pr_url.trim();

		if pr_url.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` requires a non-empty `pr_url`.",
			));
		}

		let summary = tracker_tool_bridge::normalize_summary(&parsed.summary);

		if summary.is_empty() {
			return DynamicToolCallResponse::failure(String::from(
				"`issue_closeout_complete` requires a non-empty `summary`.",
			));
		}

		let pull_request = match self.validate_closeout_pr(review_context, pr_url) {
			Ok(pull_request) => pull_request,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) = self.validate_closeout_issue_completed_state() {
			return DynamicToolCallResponse::failure(error);
		}
		if let Err(error) = self.append_review_completion_intent(
			review_context,
			RunCompletionDisposition::Closeout,
			&pull_request,
			&summary,
		) {
			return DynamicToolCallResponse::failure(error);
		}

		self.pending_review_completion.borrow_mut().replace(PendingReviewCompletion::Closeout(
			PendingReviewAction { pr_url: pull_request.url.clone(), summary },
		));

		DynamicToolCallResponse::success(format!(
			"Recorded retained closeout completion for issue `{}` on merged PR `{}`. Decodex will validate the merged lineage and terminal tracker state before cleaning up the lane.",
			self.issue.identifier, pull_request.url
		))
	}

	pub(in crate::agent::tracker_tool_bridge::tools) fn handle_terminal_finalize(
		&self,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<TerminalFinalizeArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.terminal_finalize` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let requested_path = match parsed.path.as_str() {
			"review_handoff" => RunCompletionDisposition::ReviewHandoff,
			"review_repair" => RunCompletionDisposition::ReviewRepair,
			"closeout" => RunCompletionDisposition::Closeout,
			"manual_attention" => RunCompletionDisposition::ManualAttention,
			other => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` path must be `review_handoff`, `review_repair`, `closeout`, or `manual_attention`, not `{other}`."
				));
			},
		};
		let actual_path = match self.completion_disposition() {
			Ok(actual_path) => actual_path,
			Err(error) => return DynamicToolCallResponse::failure(error.to_string()),
		};

		if requested_path != actual_path {
			return DynamicToolCallResponse::failure(format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` requested path `{}`, but the recorded terminal path is `{}`.",
				requested_path.as_str(),
				actual_path.as_str()
			));
		}

		let Some(review_context) = self.review_context.as_ref() else {
			return DynamicToolCallResponse::failure(format!(
				"`{ISSUE_TERMINAL_FINALIZE_TOOL_NAME}` is unavailable for this run."
			));
		};

		if let Err(error) = self.ensure_docs_impact_checkpoint(review_context, actual_path) {
			return DynamicToolCallResponse::failure(error);
		}

		if actual_path == RunCompletionDisposition::ReviewHandoff
			&& let Err(error) = self.persist_terminal_review_handoff_marker(review_context)
		{
			return DynamicToolCallResponse::failure(error);
		}

		if let Err(error) = self.append_terminal_finalize_event(review_context, actual_path) {
			return DynamicToolCallResponse::failure(error);
		}

		self.finalized_completion_path.replace(Some(actual_path));

		DynamicToolCallResponse::success(format!(
			"Finalized terminal path `{}` for issue `{}`. You can only finish the turn after this succeeds.",
			actual_path.as_str(),
			self.issue.identifier
		))
	}
}
