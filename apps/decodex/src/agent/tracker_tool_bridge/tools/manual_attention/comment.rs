use serde_json::{self, Value};

use crate::{
	agent::tracker_tool_bridge::{
		CommentArgs, DynamicToolCallResponse, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME,
		TrackerToolBridge,
		tools::{COMMENT_KIND_MANUAL_ATTENTION, manual_attention},
	},
	tracker,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools) fn handle_comment(
		&self,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<CommentArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.comment` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		match parsed.kind.as_str() {
			COMMENT_KIND_MANUAL_ATTENTION => self.handle_manual_attention_comment(parsed),
			other => DynamicToolCallResponse::failure(format!(
				"Unsupported `{ISSUE_COMMENT_TOOL_NAME}` kind `{other}`. Supported kinds: `{COMMENT_KIND_MANUAL_ATTENTION}`."
			)),
		}
	}

	fn handle_manual_attention_comment(&self, parsed: CommentArgs) -> DynamicToolCallResponse {
		if !*self.manual_attention_requested.borrow() {
			return DynamicToolCallResponse::failure(format!(
				"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires a successful `{ISSUE_LABEL_ADD_TOOL_NAME}` call for label `{}` before writing the explanatory comment.",
				self.workflow.frontmatter().tracker().needs_attention_label()
			));
		}

		let review_context = match self.review_context.as_ref() {
			Some(review_context) => review_context,
			None => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires an active Decodex run context."
				));
			},
		};
		let state_store = match self.state_store {
			Some(state_store) => state_store,
			None => {
				return DynamicToolCallResponse::failure(format!(
					"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires the Decodex runtime state store for issue `{}`.",
					self.issue.identifier
				));
			},
		};
		let comment = match manual_attention::normalize_manual_attention_comment(parsed) {
			Ok(comment) => comment,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Some(decision_request) = comment.decision_request.as_ref()
			&& let Err(error) = self.append_private_authority_decision_request(
				review_context,
				state_store,
				decision_request,
			) {
			return DynamicToolCallResponse::failure(error);
		}

		let record = self.manual_attention_execution_event(review_context, &comment);
		let body = manual_attention::format_manual_attention_comment(review_context, &comment);
		let projection = match tracker::prepare_linear_execution_event_comment(
			&body,
			&record,
			self.public_projection_privacy_classifier,
		) {
			Ok(projection) => projection,
			Err(error) => return DynamicToolCallResponse::failure(error.to_string()),
		};

		if let Err(error) = self.apply_manual_attention_label() {
			return DynamicToolCallResponse::failure(error);
		}

		match tracker::create_prepared_linear_execution_event_comment(
			self.tracker,
			&self.issue.id,
			&projection,
		) {
			Ok(created) => {
				if let Err(error) = state_store.record_linear_execution_event(&projection.record) {
					return DynamicToolCallResponse::failure(format!(
						"Failed to persist the public manual-attention summary for issue `{}`: {error}",
						self.issue.identifier
					));
				}

				self.manual_attention_comment_recorded.replace(true);
				self.manual_attention_error_class.replace(Some(comment.error_class.clone()));

				let verb = if created { "added" } else { "already existed for" };

				DynamicToolCallResponse::success(format!(
					"Manual-attention public summary {verb} issue `{}`.",
					self.issue.identifier
				))
			},
			Err(error) => DynamicToolCallResponse::failure(format!(
				"Failed to add a manual-attention public summary to issue `{}`: {error}",
				self.issue.identifier
			)),
		}
	}
}
