use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use crate::{
	prelude::{Result, eyre},
	workflow::WorkflowWorkspaceHooks,
	worktree::{WorktreeManager, WorktreeSpec, git, hooks},
};

impl WorktreeManager {
	#[cfg(test)]
	pub(crate) fn ensure_worktree(
		&self,
		issue_identifier: &str,
		dry_run: bool,
	) -> Result<WorktreeSpec> {
		self.ensure_worktree_internal(issue_identifier, dry_run, None, None)
	}

	pub(crate) fn ensure_worktree_with_hooks(
		&self,
		issue_identifier: &str,
		dry_run: bool,
		hooks: &WorkflowWorkspaceHooks,
	) -> Result<WorktreeSpec> {
		self.ensure_worktree_internal(issue_identifier, dry_run, Some(hooks), None)
	}

	pub(crate) fn ensure_worktree_with_hooks_at_base(
		&self,
		issue_identifier: &str,
		dry_run: bool,
		hooks: &WorkflowWorkspaceHooks,
		admitted_base_oid: &str,
	) -> Result<WorktreeSpec> {
		self.ensure_worktree_internal(
			issue_identifier,
			dry_run,
			Some(hooks),
			Some(admitted_base_oid),
		)
	}

	pub(crate) fn source_head_oid(&self) -> Result<String> {
		git::git_stdout(
			&self.repo_root,
			["rev-parse", "HEAD"],
			"read the source repository HEAD",
		)
	}

	fn ensure_worktree_internal(
		&self,
		issue_identifier: &str,
		dry_run: bool,
		hooks: Option<&WorkflowWorkspaceHooks>,
		admitted_base_oid: Option<&str>,
	) -> Result<WorktreeSpec> {
		let spec = self.plan_for_issue(issue_identifier);

		if dry_run {
			return Ok(spec);
		}
		if spec.reused_existing {
			self.validate_worktree_boundary(&spec.path)?;
			if let Some(base_oid) = admitted_base_oid {
				git::run_git(
					&spec.path,
					["merge-base", "--is-ancestor", base_oid, "HEAD"],
					"verify the frozen admitted base is an ancestor of the retained lane head",
				)?;
			}

			git::normalize_origin_remote_for_worktrees(&self.repo_root)?;

			self.resume_after_create_hooks_if_pending(&spec, hooks)?;

			return Ok(spec);
		}

		fs::create_dir_all(&self.worktree_root)?;

		let source_head = match admitted_base_oid {
			Some(oid) => oid.to_owned(),
			None => self.source_head_oid()?,
		};
		self.create_linked_worktree(&spec, hooks, &source_head)?;
		self.validate_worktree_boundary(&spec.path)?;
		self.run_after_create_hooks(&spec, hooks)?;

		Ok(spec)
	}

	pub(super) fn create_linked_worktree(
		&self,
		spec: &WorktreeSpec,
		hooks: Option<&WorkflowWorkspaceHooks>,
		source_head: &str,
	) -> Result<()> {
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
			.arg(source_head)
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

		let setup_result =
			git::normalize_origin_remote_for_worktrees(&self.repo_root).and_then(|_| {
				self.checkout_worktree_branch(&spec.path, spec.branch_name.as_str(), source_head)?;
				git::run_git(
					&spec.path,
					["merge-base", "--is-ancestor", source_head, "HEAD"],
					"verify the lane head descends from its frozen admitted base",
				)
			});

		if let Err(error) = setup_result {
			let _ = self.remove_worktree_path_internal(&spec.path, None);

			return Err(error);
		}

		if hooks::workspace_requires_after_create_pending_marker(hooks) {
			let pending_marker = hooks::after_create_pending_marker_path(&spec.path);

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
		if git::fetch_remote_branch_if_present(&self.repo_root, branch_name)? {
			let remote_tracking_ref = format!("refs/remotes/origin/{branch_name}");

			git::run_git(
				worktree_path,
				["checkout", "--quiet", "-B", branch_name, remote_tracking_ref.as_str()],
				"checkout the worktree branch from the remote lane head",
			)?;
		} else {
			git::run_git(
				worktree_path,
				["checkout", "--quiet", "-B", branch_name, source_head],
				"checkout the worktree branch",
			)?;
		}

		Ok(())
	}

	pub(super) fn validate_worktree_boundary(&self, worktree_path: &Path) -> Result<()> {
		let git_pointer = worktree_path.join(".git");

		if !git_pointer.is_file() {
			eyre::bail!(
				"Worktree `{}` is not a linked git worktree: expected `.git` to be a pointer file.",
				worktree_path.display()
			);
		}

		let repo_git_dir = git::resolve_source_repo_git_common_dir(&self.repo_root)?;
		let worktree_admin_root = repo_git_dir.join("worktrees");
		let canonical_worktree_path = fs::canonicalize(worktree_path)?;
		let git_dir = fs::canonicalize(PathBuf::from(git::git_stdout(
			worktree_path,
			["rev-parse", "--path-format=absolute", "--git-dir"],
			"resolve worktree git dir",
		)?))?;
		let git_common_dir = fs::canonicalize(PathBuf::from(git::git_stdout(
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
		if !git::worktree_is_registered(&self.repo_root, &canonical_worktree_path)? {
			eyre::bail!(
				"Worktree `{}` is not registered with the source repository worktree admin.",
				worktree_path.display()
			);
		}

		Ok(())
	}
}
