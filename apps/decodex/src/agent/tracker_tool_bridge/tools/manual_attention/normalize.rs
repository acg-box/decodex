mod decision;
mod fields;
mod public_text;
mod runtime_owned;

use crate::agent::tracker_tool_bridge::{
	self, CommentArgs, ISSUE_COMMENT_TOOL_NAME,
	tools::{COMMENT_KIND_MANUAL_ATTENTION, manual_attention::NormalizedManualAttentionComment},
};

pub(super) fn normalize_manual_attention_comment(
	parsed: CommentArgs,
) -> Result<NormalizedManualAttentionComment, String> {
	let error_class = fields::normalize_required_comment_field(parsed.error_class, "error_class")?;
	let next_action = fields::normalize_required_comment_field(parsed.next_action, "next_action")?;
	let blockers = tracker_tool_bridge::normalize_progress_list(parsed.blockers);
	let evidence = tracker_tool_bridge::normalize_progress_list(parsed.evidence);
	let failed_command =
		tracker_tool_bridge::normalize_optional_progress_field(parsed.failed_command);
	let raw_error = tracker_tool_bridge::normalize_optional_progress_field(parsed.raw_error);
	let summary = tracker_tool_bridge::normalize_optional_progress_field(parsed.summary);
	let decision_request =
		parsed.decision_request.map(decision::normalize_authority_decision_request).transpose()?;

	public_text::validate_manual_attention_error_class(&error_class)?;

	if blockers.is_empty() {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `blockers` item."
		));
	}
	if evidence.is_empty() {
		return Err(format!(
			"`{ISSUE_COMMENT_TOOL_NAME}` kind `{COMMENT_KIND_MANUAL_ATTENTION}` requires at least one public `evidence` item."
		));
	}

	Ok(NormalizedManualAttentionComment {
		error_class,
		next_action,
		blockers,
		evidence,
		failed_command,
		raw_error,
		summary,
		decision_request,
	})
}
