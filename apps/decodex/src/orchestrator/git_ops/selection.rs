use std::path::Path;

use super::repo_gate_changed_tracked_files;
use crate::workflow::{ResolvedRepoGate, WorkflowExecution};

pub(crate) fn select_repo_gate_for_worktree<'a>(
	execution: &'a WorkflowExecution,
	cwd: &Path,
) -> ResolvedRepoGate<'a> {
	if execution.gate_profiles().is_empty() {
		return execution.default_repo_gate();
	}

	let changed_files = match repo_gate_changed_tracked_files(cwd) {
		Ok(changed_files) => changed_files,
		Err(error) => {
			tracing::warn!(
				repo_root = %cwd.display(),
				error = %error,
				"Falling back to the default full repo gate because changed-file classification was unavailable."
			);

			return execution.default_repo_gate();
		},
	};
	let selected_gate = execution.select_repo_gate_for_changed_files(&changed_files);

	if let Some(profile_name) = selected_gate.profile_name() {
		tracing::info!(
			repo_root = %cwd.display(),
			profile_name,
			changed_file_count = changed_files.len(),
			"Selected a narrowed repo gate profile from changed tracked files."
		);
	}

	selected_gate
}
