use std::borrow::Cow;

use crate::{
	agent::tracker_tool_bridge::{self, PendingReviewAction, ReviewHandoffContext},
	tracker::public_text,
};

pub(super) fn normalize_summary(summary: &str) -> String {
	summary.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn normalize_progress_list(items: Vec<String>) -> Vec<String> {
	items.into_iter().map(|item| normalize_summary(&item)).filter(|item| !item.is_empty()).collect()
}

pub(super) fn normalize_optional_progress_field(value: Option<String>) -> Option<String> {
	value.and_then(|value| {
		let normalized = normalize_summary(&value);

		(!normalized.is_empty()).then_some(normalized)
	})
}

pub(super) fn public_summary_or_fallback<'a>(
	summary: &'a str,
	fallback: &'static str,
) -> Cow<'a, str> {
	if public_text::validate_public_text_field("summary", summary).is_ok() {
		Cow::Borrowed(summary)
	} else {
		Cow::Borrowed(fallback)
	}
}

pub(super) fn format_review_handoff_comment(
	review_context: &ReviewHandoffContext,
	pending_review_handoff: &PendingReviewAction,
	summary: &str,
) -> String {
	format!(
		"decodex run completed and is ready for review\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt}` (not retry-budget count)\n- finished_at: `{finished_at}`\n- branch: `{branch}`\n- pr_url: `{pr_url}`\n- worktree_path: `{worktree_path}`\n- validation_result: `passed`\n- summary: {summary}",
		run_id = review_context.run_id,
		attempt = review_context.attempt_number,
		finished_at = tracker_tool_bridge::current_timestamp(),
		branch = review_context.branch_name,
		pr_url = pending_review_handoff.pr_url,
		worktree_path = review_context.worktree_path,
		summary = summary,
	)
}

pub(super) fn format_review_repair_comment(
	review_context: &ReviewHandoffContext,
	pending_review_repair: &PendingReviewAction,
	summary: &str,
) -> String {
	format!(
		"decodex review repair completed and requested fresh review\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt}` (not retry-budget count)\n- finished_at: `{finished_at}`\n- branch: `{branch}`\n- pr_url: `{pr_url}`\n- worktree_path: `{worktree_path}`\n- validation_result: `passed`\n- summary: {summary}",
		run_id = review_context.run_id,
		attempt = review_context.attempt_number,
		finished_at = tracker_tool_bridge::current_timestamp(),
		branch = review_context.branch_name,
		pr_url = pending_review_repair.pr_url,
		worktree_path = review_context.worktree_path,
		summary = summary,
	)
}
