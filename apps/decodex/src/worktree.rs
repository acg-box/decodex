use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	process::Command,
	time::Duration,
};

use crate::{
	prelude::{Result, eyre},
	workflow::WorkflowWorkspaceHooks,
};

mod cleanup;
mod git;
mod hooks;

#[allow(unused_imports)] pub(crate) use cleanup::MergedWorktreeCleanliness;
pub(crate) use cleanup::{
	MergedWorktreeCleanupDebt, infer_default_branch_name, merged_worktree_cleanup_debts,
};
#[cfg(test)] use git::is_relative_filesystem_remote;
use git::{
	configured_branch_owner, fetch_remote_branch_if_present, git_stdout,
	normalize_origin_remote_for_worktrees, resolve_source_repo_git_common_dir, run_git,
	sanitize_branch_component, worktree_is_registered,
};
#[cfg(test)] use hooks::workspace_hook_shell_from_env;
use hooks::{
	after_create_pending_marker_path, append_output_details,
	remove_orphan_marker_directory_if_safe, run_workspace_hook_shell_command,
	workspace_requires_after_create_pending_marker,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorktreeSpec {
	pub(crate) branch_name: String,
	pub(crate) issue_identifier: String,
	pub(crate) path: PathBuf,
	pub(crate) reused_existing: bool,
}

pub(crate) struct WorktreeManager {
	repo_root: PathBuf,
	worktree_root: PathBuf,
	project_id: String,
}
impl WorktreeManager {
	pub(crate) fn new(
		project_id: impl Into<String>,
		repo_root: impl Into<PathBuf>,
		worktree_root: impl Into<PathBuf>,
	) -> Self {
		Self {
			repo_root: repo_root.into(),
			worktree_root: worktree_root.into(),
			project_id: project_id.into(),
		}
	}

	pub(crate) fn plan_for_issue(&self, issue_identifier: &str) -> WorktreeSpec {
		let branch_suffix = sanitize_branch_component(issue_identifier);
		let branch_owner =
			configured_branch_owner(&self.repo_root).unwrap_or_else(|| String::from("x"));
		let branch_name = format!(
			"{}/{}-{}",
			sanitize_branch_component(&branch_owner),
			sanitize_branch_component(&self.project_id),
			branch_suffix
		);
		let path = self.worktree_root.join(issue_identifier);
		let reused_existing = path.join(".git").exists();

		WorktreeSpec {
			branch_name,
			issue_identifier: issue_identifier.to_owned(),
			path,
			reused_existing,
		}
	}

	#[cfg(test)]
	pub(crate) fn ensure_worktree(
		&self,
		issue_identifier: &str,
		dry_run: bool,
	) -> Result<WorktreeSpec> {
		self.ensure_worktree_internal(issue_identifier, dry_run, None)
	}

	pub(crate) fn ensure_worktree_with_hooks(
		&self,
		issue_identifier: &str,
		dry_run: bool,
		hooks: &WorkflowWorkspaceHooks,
	) -> Result<WorktreeSpec> {
		self.ensure_worktree_internal(issue_identifier, dry_run, Some(hooks))
	}

	fn ensure_worktree_internal(
		&self,
		issue_identifier: &str,
		dry_run: bool,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<WorktreeSpec> {
		let spec = self.plan_for_issue(issue_identifier);

		if dry_run {
			return Ok(spec);
		}
		if spec.reused_existing {
			self.validate_worktree_boundary(&spec.path)?;

			normalize_origin_remote_for_worktrees(&self.repo_root)?;

			self.resume_after_create_hooks_if_pending(&spec, hooks)?;

			return Ok(spec);
		}

		fs::create_dir_all(&self.worktree_root)?;

		self.create_linked_worktree(&spec, hooks)?;
		self.validate_worktree_boundary(&spec.path)?;
		self.run_after_create_hooks(&spec, hooks)?;

		Ok(spec)
	}

	pub(crate) fn remove_worktree_path(&self, path: &Path) -> Result<bool> {
		self.remove_worktree_path_internal(path, None)
	}

	pub(crate) fn remove_worktree_path_with_hooks(
		&self,
		issue_identifier: &str,
		branch_name: &str,
		path: &Path,
		hooks: &WorkflowWorkspaceHooks,
	) -> Result<bool> {
		self.remove_worktree_path_internal(path, Some((issue_identifier, branch_name, hooks)))
	}

	fn remove_worktree_path_internal(
		&self,
		path: &Path,
		hooks: Option<(&str, &str, &WorkflowWorkspaceHooks)>,
	) -> Result<bool> {
		if !path.try_exists().map_err(|error| {
			eyre::eyre!("Failed to inspect worktree path `{}`: {error}", path.display())
		})? {
			return Ok(false);
		}

		let worktree_root = fs::canonicalize(&self.worktree_root)?;
		let canonical_path = fs::canonicalize(path)?;

		if !canonical_path.starts_with(&worktree_root) || canonical_path == worktree_root {
			eyre::bail!(
				"Refusing to remove worktree `{}` outside worktree_root `{}`.",
				path.display(),
				self.worktree_root.display()
			);
		}
		if remove_orphan_marker_directory_if_safe(&canonical_path)? {
			return Ok(true);
		}

		self.validate_worktree_boundary(&canonical_path)?;

		if let Some((issue_identifier, branch_name, hooks)) = hooks {
			self.run_workspace_hook_phase(
				"before_remove",
				issue_identifier,
				branch_name,
				&canonical_path,
				hooks.before_remove_commands(),
				hooks.timeout_seconds(),
			)?;
		}

		run_git(
			&self.repo_root,
			[
				"worktree",
				"remove",
				"--force",
				canonical_path.as_os_str().to_str().ok_or_else(|| {
					eyre::eyre!("Worktree path `{}` is not valid UTF-8.", canonical_path.display())
				})?,
			],
			"remove the linked worktree",
		)?;

		Ok(true)
	}

	fn create_linked_worktree(
		&self,
		spec: &WorktreeSpec,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<()> {
		let source_head =
			git_stdout(&self.repo_root, ["rev-parse", "HEAD"], "read the source repository HEAD")?;

		if spec.path.exists() {
			eyre::bail!(
				"Worktree path `{}` already exists but does not look reusable.",
				spec.path.display()
			);
		}

		let create_output = Command::new("git")
			.arg("-C")
			.arg(&self.repo_root)
			.args(["worktree", "add", "--quiet", "--detach"])
			.arg(&spec.path)
			.arg(&source_head)
			.output()?;

		if !create_output.status.success() {
			let stderr = String::from_utf8_lossy(&create_output.stderr);

			eyre::bail!(
				"Failed to create linked worktree `{}` from `{}`: {}",
				spec.path.display(),
				self.repo_root.display(),
				stderr.trim()
			);
		}

		let setup_result = normalize_origin_remote_for_worktrees(&self.repo_root).and_then(|_| {
			self.checkout_worktree_branch(&spec.path, spec.branch_name.as_str(), &source_head)
		});

		if let Err(error) = setup_result {
			let _ = self.remove_worktree_path_internal(&spec.path, None);

			return Err(error);
		}

		if workspace_requires_after_create_pending_marker(hooks) {
			let pending_marker = after_create_pending_marker_path(&spec.path);

			fs::write(&pending_marker, b"pending\n").map_err(|error| {
				let _ = self.remove_worktree_path_internal(&spec.path, None);

				eyre::eyre!(
					"Failed to write pending after-create marker `{}`: {error}",
					pending_marker.display()
				)
			})?;
		}

		Ok(())
	}

	fn checkout_worktree_branch(
		&self,
		worktree_path: &Path,
		branch_name: &str,
		source_head: &str,
	) -> Result<()> {
		if fetch_remote_branch_if_present(&self.repo_root, branch_name)? {
			let remote_tracking_ref = format!("refs/remotes/origin/{branch_name}");

			run_git(
				worktree_path,
				["checkout", "--quiet", "-B", branch_name, remote_tracking_ref.as_str()],
				"checkout the worktree branch from the remote lane head",
			)?;
		} else {
			run_git(
				worktree_path,
				["checkout", "--quiet", "-B", branch_name, source_head],
				"checkout the worktree branch",
			)?;
		}

		Ok(())
	}

	fn validate_worktree_boundary(&self, worktree_path: &Path) -> Result<()> {
		let git_pointer = worktree_path.join(".git");

		if !git_pointer.is_file() {
			eyre::bail!(
				"Worktree `{}` is not a linked git worktree: expected `.git` to be a pointer file.",
				worktree_path.display()
			);
		}

		let repo_git_dir = resolve_source_repo_git_common_dir(&self.repo_root)?;
		let worktree_admin_root = repo_git_dir.join("worktrees");
		let canonical_worktree_path = fs::canonicalize(worktree_path)?;
		let git_dir = fs::canonicalize(PathBuf::from(git_stdout(
			worktree_path,
			["rev-parse", "--path-format=absolute", "--git-dir"],
			"resolve worktree git dir",
		)?))?;
		let git_common_dir = fs::canonicalize(PathBuf::from(git_stdout(
			worktree_path,
			["rev-parse", "--path-format=absolute", "--git-common-dir"],
			"resolve worktree git common dir",
		)?))?;

		if !git_dir.starts_with(&worktree_admin_root) {
			eyre::bail!(
				"Worktree `{}` is not linked through `{}`: git dir resolved to `{}`.",
				worktree_path.display(),
				worktree_admin_root.display(),
				git_dir.display()
			);
		}
		if git_common_dir != repo_git_dir {
			eyre::bail!(
				"Worktree `{}` must share git common dir `{}`, found `{}`.",
				worktree_path.display(),
				repo_git_dir.display(),
				git_common_dir.display()
			);
		}
		if !worktree_is_registered(&self.repo_root, &canonical_worktree_path)? {
			eyre::bail!(
				"Worktree `{}` is not registered with the source repository worktree admin.",
				worktree_path.display()
			);
		}

		Ok(())
	}

	fn run_workspace_hook_phase(
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
			let output = run_workspace_hook_shell_command(
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

				append_output_details(&mut details, &output);

				eyre::bail!(
					"Workspace hook `{phase_name}` command `{command}` failed in `{}` with status `{}`.{details}",
					worktree_path.display(),
					output.status
				);
			}
		}

		Ok(())
	}

	fn run_after_create_hooks(
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

		let pending_marker = after_create_pending_marker_path(&spec.path);

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

	fn resume_after_create_hooks_if_pending(
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
		if !after_create_pending_marker_path(&spec.path).exists() {
			return Ok(());
		}

		self.run_after_create_hooks(spec, Some(hooks))
	}
}

#[cfg(test)] mod tests;
