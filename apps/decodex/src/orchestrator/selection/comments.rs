use crate::orchestrator::selection::{model::RetryComment, runtime};

pub(crate) fn format_retry_comment(comment: RetryComment<'_>) -> String {
	let RetryComment {
		run_id,
		attempt_number,
		retry_budget_attempt_number,
		max_attempts,
		worktree_path,
		branch_name,
		error_class,
		next_action,
	} = comment;

	format!(
		"decodex run failed and will retry\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt_number}` (not retry-budget count)\n- retry_budget_attempt: `{retry_budget_attempt_number}` / `{max_attempts}`\n- failed_at: `{failed_at}`\n- branch: `{branch}`\n- worktree_path: `{worktree}`\n- error_class: `{error_class}`\n- next_action: `{next_action}`\n- error_summary: `Sensitive runtime details were withheld from the tracker comment; inspect the local lane for the full failure context.`",
		failed_at = runtime::current_timestamp(),
		branch = branch_name,
		worktree = worktree_path,
	)
}

pub(crate) fn format_terminal_failure_comment(
	run_id: &str,
	attempt_number: i64,
	worktree_path: String,
	branch_name: &str,
	pr_url: Option<&str>,
	error_class: &str,
	next_action: &str,
) -> String {
	let pr_url_line = pr_url.map_or_else(String::new, |pr_url| format!("\n- pr_url: `{pr_url}`"));
	let retained_partial_progress = error_class == "partial_progress_retained";
	let heading = if retained_partial_progress {
		"decodex retained partial progress and needs attention"
	} else {
		"decodex run failed and needs attention"
	};
	let timestamp_label = if retained_partial_progress { "recorded_at" } else { "failed_at" };
	let error_summary = if retained_partial_progress {
		"Sensitive runtime details were withheld from the tracker comment; inspect the retained lane for the full recovery context."
	} else {
		"Sensitive runtime details were withheld from the tracker comment; inspect the local lane for the full failure context."
	};

	format!(
		"{heading}\n\n- run_id: `{run_id}`\n- run_sequence_attempt: `{attempt_number}` (not retry-budget count)\n- {timestamp_label}: `{timestamp}`\n- branch: `{branch}`{pr_url_line}\n- worktree_path: `{worktree}`\n- error_class: `{error_class}`\n- next_action: `{next_action}`\n- error_summary: `{error_summary}`",
		timestamp = runtime::current_timestamp(),
		branch = branch_name,
		worktree = worktree_path
	)
}
