use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn review_policy_mutation_fence(
		&self,
		tool_name: &str,
	) -> Option<DynamicToolCallResponse> {
		if matches!(
			tool_name,
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME | ISSUE_TERMINAL_FINALIZE_TOOL_NAME
		) {
			return None;
		}

		let review_context = self.review_context.as_ref()?;

		match self.review_policy_stop_requested(review_context) {
			Ok(Some(stop)) => Some(DynamicToolCallResponse::failure(format!(
				"Review policy stop `{}` is active for issue `{}` after `{}` non-clean rounds; `{tool_name}` is fenced until architecture recovery or human attention resolves the lane.",
				stop.reason.error_class(),
				stop.issue_identifier,
				stop.nonclean_rounds.unwrap_or_default()
			))),
			Ok(None) => None,
			Err(error) => Some(DynamicToolCallResponse::failure(format!(
				"Failed to evaluate review policy mutation fence for `{tool_name}`: {error}"
			))),
		}
	}
}
