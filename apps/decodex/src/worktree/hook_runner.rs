use std::{fs, io::ErrorKind, path::Path, time::Duration};

use crate::{
	prelude::{Result, eyre},
	workflow::WorkflowWorkspaceHooks,
	worktree::{WorktreeManager, WorktreeSpec, hooks},
};

impl WorktreeManager {
	pub(super) fn run_workspace_hook_phase(
		&self,
		phase_name: &str,
		issue_identifier: &str,
		branch_name: &str,
		worktree_path: &Path,
		commands: &[String],
		timeout_seconds: u64,
	) -> Result<()> {
		if commands.is_empty() {
			return Ok(());
		}

		let envs = [
			("DECODEX_REPO_ROOT", self.repo_root.display().to_string()),
			("DECODEX_WORKTREE_PATH", worktree_path.display().to_string()),
			("DECODEX_ISSUE_ID", issue_identifier.to_owned()),
			("DECODEX_BRANCH", branch_name.to_owned()),
		];

		for command in commands {
			let output = hooks::run_workspace_hook_shell_command(
				command,
				worktree_path,
				&envs,
				Duration::from_secs(timeout_seconds),
			)
			.map_err(|error| {
				eyre::eyre!(
					"Failed to run workspace hook `{phase_name}` command `{command}` in `{}`: {error}",
					worktree_path.display()
				)
			})?;

			if !output.status.success() {
				let mut details = String::new();

				hooks::append_output_details(&mut details, &output);

				eyre::bail!(
					"Workspace hook `{phase_name}` command `{command}` failed in `{}` with status `{}`.{details}",
					worktree_path.display(),
					output.status
				);
			}
		}

		Ok(())
	}

	pub(super) fn run_after_create_hooks(
		&self,
		spec: &WorktreeSpec,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<()> {
		let Some(hooks) = hooks else {
			return Ok(());
		};

		if hooks.after_create_commands().is_empty() {
			return Ok(());
		}

		let pending_marker = hooks::after_create_pending_marker_path(&spec.path);

		if let Err(error) = self.run_workspace_hook_phase(
			"after_create",
			spec.issue_identifier.as_str(),
			spec.branch_name.as_str(),
			&spec.path,
			hooks.after_create_commands(),
			hooks.timeout_seconds(),
		) {
			fs::write(&pending_marker, b"pending\n").map_err(|marker_error| {
				eyre::eyre!(
					"Workspace after-create hook failed and pending marker `{}` could not be restored: {marker_error}. Original error: {error}",
					pending_marker.display()
				)
			})?;

			return Err(error);
		}
		if let Err(error) = fs::remove_file(&pending_marker)
			&& error.kind() != ErrorKind::NotFound
		{
			return Err(eyre::eyre!(
				"Failed to clear pending after-create marker `{}` after successful bootstrap: {error}",
				pending_marker.display()
			));
		}

		Ok(())
	}

	pub(super) fn resume_after_create_hooks_if_pending(
		&self,
		spec: &WorktreeSpec,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<()> {
		let Some(hooks) = hooks else {
			return Ok(());
		};

		if hooks.after_create_commands().is_empty() {
			return Ok(());
		}
		if !hooks::after_create_pending_marker_path(&spec.path).exists() {
			return Ok(());
		}

		self.run_after_create_hooks(spec, Some(hooks))
	}
}
