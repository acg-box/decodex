use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, RunCompletionDisposition,
	TerminalFinalizeArgs, TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
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
			&& let Err(error) = self.persist_terminal_review_lifecycle_handoff(review_context)
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
