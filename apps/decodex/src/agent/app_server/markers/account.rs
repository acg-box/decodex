use std::path::Path;

use crate::{
	agent::app_server::{CodexAccountActivitySummary, CodexAccountMarker},
	state,
};

pub(in crate::agent::app_server) fn write_codex_account_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	summary: &CodexAccountActivitySummary,
	account_summaries: &[CodexAccountActivitySummary],
) {
	if let Err(error) = state::write_run_account_marker(
		marker_path,
		&CodexAccountMarker {
			run_id,
			attempt_number,
			account: summary,
			accounts: account_summaries,
		},
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree Codex account marker."
		);
	}
}
