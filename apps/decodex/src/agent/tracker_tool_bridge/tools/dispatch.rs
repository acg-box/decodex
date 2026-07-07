mod fence;
mod label;
mod transition;

use serde_json::Value;

use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, ISSUE_COMMENT_TOOL_NAME, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
	ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
	ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
	ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
	ISSUE_TRANSITION_TOOL_NAME, TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn handle_call_inner(
		&self,
		tool_name: &str,
		arguments: Value,
	) -> DynamicToolCallResponse {
		if let Some(response) = self.review_policy_mutation_fence(tool_name) {
			return response;
		}

		match tool_name {
			ISSUE_TRANSITION_TOOL_NAME => self.handle_transition(arguments),
			ISSUE_COMMENT_TOOL_NAME => self.handle_comment(arguments),
			ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME => self.handle_progress_checkpoint(arguments),
			ISSUE_REVIEW_CHECKPOINT_TOOL_NAME => self.handle_review_checkpoint(arguments),
			ISSUE_REVIEW_HANDOFF_TOOL_NAME => self.handle_review_handoff(arguments),
			ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME => self.handle_review_repair_complete(arguments),
			ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME => self.handle_closeout_complete(arguments),
			ISSUE_LABEL_ADD_TOOL_NAME => self.handle_add_label(arguments),
			ISSUE_TERMINAL_FINALIZE_TOOL_NAME => self.handle_terminal_finalize(arguments),
			_ => {
				DynamicToolCallResponse::failure(format!("Unsupported tracker tool `{tool_name}`."))
			},
		}
	}
}
