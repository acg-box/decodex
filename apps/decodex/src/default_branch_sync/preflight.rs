use std::{collections::BTreeSet, path::Path};

use crate::{
	default_branch_sync::{commands, credentials},
	git_credentials::{GitCredentialEnvironment, GitCredentialSource},
	prelude::{Result, eyre},
};

pub(crate) fn sync_repo_root_default_branch(
	repo_root: &Path,
	default_branch: &str,
	credentials: Option<GitCredentialSource<'_>>,
) -> Result<()> {
	let git_env = credentials::materialize_git_credentials(credentials);

	preflight_repo_root_default_branch_sync_with_env(repo_root, default_branch, &git_env)?;

	fast_forward_default_branch(repo_root, default_branch, &git_env)
}

pub(crate) fn preflight_repo_root_default_branch_sync(
	repo_root: &Path,
	default_branch: &str,
	credentials: Option<GitCredentialSource<'_>>,
) -> Result<()> {
	let git_env = credentials::materialize_git_credentials(credentials);

	preflight_repo_root_default_branch_sync_with_env(repo_root, default_branch, &git_env)
}

fn preflight_repo_root_default_branch_sync_with_env(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let current_branch =
		commands::run_git_capture(repo_root, &["branch", "--show-current"], git_env)?;

	if current_branch.is_empty() {
		eyre::bail!(
			"Configured repo root `{}` is detached; landing closeout cannot fast-forward local `{default_branch}` there.",
			repo_root.display()
		);
	}
	if current_branch != default_branch {
		eyre::bail!(
			"Configured repo root `{}` is on branch `{current_branch}`, but landing closeout must fast-forward local `{default_branch}` there.",
			repo_root.display()
		);
	}

	ensure_clean_default_branch_worktree(repo_root, default_branch, git_env)?;
	fetch_default_branch(repo_root, default_branch, git_env)?;
	ensure_no_untracked_overwrite_conflicts(repo_root, default_branch, git_env)?;
	ensure_fast_forward_possible(repo_root, default_branch, git_env)?;

	Ok(())
}

fn ensure_clean_default_branch_worktree(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let status = commands::run_git_capture(
		repo_root,
		&["status", "--porcelain", "--untracked-files=no"],
		git_env,
	)?;

	if status.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"Configured repo root `{}` has tracked local changes; landing closeout cannot fast-forward local `{default_branch}` until they are cleared.",
		repo_root.display()
	);
}

fn fetch_default_branch(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let refspec = format!("refs/heads/{default_branch}:refs/remotes/origin/{default_branch}");

	commands::run_git_checked(
		repo_root,
		&["fetch", "origin", refspec.as_str()],
		format!("fetch the latest `{default_branch}` from `origin`"),
		git_env,
	)
}

fn ensure_no_untracked_overwrite_conflicts(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let untracked_files = commands::run_git_capture(
		repo_root,
		&["ls-files", "--others", "--exclude-standard"],
		git_env,
	)?;

	if untracked_files.is_empty() {
		return Ok(());
	}

	let tracking_ref = format!("refs/remotes/origin/{default_branch}");
	let incoming_paths = commands::run_git_capture(
		repo_root,
		&["diff", "--name-only", "--diff-filter=ACMRTUXB", "HEAD", tracking_ref.as_str()],
		git_env,
	)?;

	if incoming_paths.is_empty() {
		return Ok(());
	}

	let incoming_paths = incoming_paths.lines().collect::<BTreeSet<_>>();
	let conflicting_paths = untracked_files
		.lines()
		.filter(|untracked_path| {
			incoming_paths.iter().any(|incoming_path| paths_conflict(untracked_path, incoming_path))
		})
		.map(str::to_owned)
		.collect::<Vec<_>>();

	if conflicting_paths.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"Configured repo root `{}` has untracked local files that would be overwritten by fast-forwarding `{default_branch}`: {}.",
		repo_root.display(),
		conflicting_paths.join(", ")
	);
}

fn paths_conflict(left: &str, right: &str) -> bool {
	left == right
		|| left.strip_prefix(right).is_some_and(|suffix| suffix.starts_with('/'))
		|| right.strip_prefix(left).is_some_and(|suffix| suffix.starts_with('/'))
}

fn ensure_fast_forward_possible(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let tracking_ref = format!("refs/remotes/origin/{default_branch}");
	let status = commands::build_git_command(
		repo_root,
		&["merge-base", "--is-ancestor", "HEAD", tracking_ref.as_str()],
		git_env,
	)
	.status()?;

	if status.success() {
		return Ok(());
	}
	if status.code() == Some(1) {
		eyre::bail!(
			"Configured repo root `{}` cannot fast-forward local `{default_branch}` to `{tracking_ref}` because local `{default_branch}` contains commits that are not on origin.",
			repo_root.display()
		);
	}

	eyre::bail!(
		"`git merge-base --is-ancestor HEAD {tracking_ref}` failed in `{}` with status `{}`.",
		repo_root.display(),
		status
	);
}

fn fast_forward_default_branch(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let tracking_ref = format!("refs/remotes/origin/{default_branch}");

	commands::run_git_checked(
		repo_root,
		&["merge", "--ff-only", tracking_ref.as_str()],
		format!("fast-forward local `{default_branch}` to `{tracking_ref}`"),
		git_env,
	)
}
