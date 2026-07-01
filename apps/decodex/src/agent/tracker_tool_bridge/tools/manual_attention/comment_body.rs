use crate::agent::tracker_tool_bridge::tools::{
	COMMENT_KIND_MANUAL_ATTENTION, manual_attention::NormalizedManualAttentionComment,
};
use crate::agent::tracker_tool_bridge::{self, ReviewHandoffContext};

pub(super) fn format_manual_attention_comment(
	review_context: &ReviewHandoffContext,
	comment: &NormalizedManualAttentionComment,
) -> String {
	let mut lines = vec![
		String::from("decodex run needs manual attention"),
		String::new(),
		format!("- run_id: `{}`", review_context.run_id),
		format!(
			"- run_sequence_attempt: `{}` (not retry-budget count)",
			review_context.attempt_number
		),
		format!("- reported_at: `{}`", tracker_tool_bridge::current_timestamp()),
		format!("- branch: `{}`", review_context.branch_name),
		format!("- worktree_path: `{}`", review_context.worktree_path),
		format!("- comment_kind: `{COMMENT_KIND_MANUAL_ATTENTION}`"),
		format!("- error_class: `{}`", comment.error_class),
		format!("- next_action: {}", comment.next_action),
	];

	if let Some(summary) = comment.summary.as_deref() {
		lines.push(format!("- summary: {summary}"));
	}

	for blocker in &comment.blockers {
		lines.push(format!("- blocker: {blocker}"));
	}
	for evidence in &comment.evidence {
		lines.push(format!("- evidence: {evidence}"));
	}

	if let Some(request) = comment.decision_request.as_ref() {
		lines.push(String::from("- decision_request: authority_boundary"));
		lines.push(format!("- decision_request_id: `{}`", request.decision_request_id));
		lines.push(format!("- decision_reason: `{}`", request.reason_code));
		lines.push(format!("- boundary: `{}`", request.boundary_type));
		lines.push(format!("- proposed_change: {}", request.proposed_change));
		lines.push(format!("- why_exceeds_authority: {}", request.why_exceeds_authority));

		for option in &request.options {
			lines.push(format!("- decision_option: `{}` - {}", option.label, option.description));
		}

		lines.push(format!("- recommendation: {}", request.recommendation));
		lines.push(format!("- resume_condition: {}", request.resume_condition));
	}
	if let Some(failed_command) = comment.failed_command.as_deref() {
		lines.push(format!("- failed_command: {failed_command}"));
	}
	if let Some(raw_error) = comment.raw_error.as_deref() {
		lines.push(format!("- raw_error: {raw_error}"));
	}

	lines.join("\n")
}
