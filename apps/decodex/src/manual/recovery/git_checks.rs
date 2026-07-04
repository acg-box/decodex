use std::{path::Path, process::Command};

use crate::{
	manual,
	prelude::{Result, eyre},
};

pub(super) fn ensure_repo_root_default_branch_current(
	repo_root: &Path,
	default_branch: &str,
) -> Result<()> {
	let local_head = manual::run_git_capture(repo_root, &["rev-parse", "HEAD"])?;
	let tracking_ref = format!("refs/remotes/origin/{default_branch}");
	let remote_head = manual::run_git_capture(repo_root, &["rev-parse", tracking_ref.as_str()])?;

	if local_head == remote_head {
		return Ok(());
	}

	eyre::bail!(
		"Configured repo root `{}` is on `{default_branch}` but is not current with `{tracking_ref}`; sync the default branch before retrying manual land recovery.",
		repo_root.display()
	);
}

pub(super) fn ensure_merge_commit_reachable_from_default_branch(
	repo_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	default_branch: &str,
) -> Result<()> {
	let status = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["merge-base", "--is-ancestor", merge_commit, "HEAD"])
		.status()?;

	if status.success() {
		return Ok(());
	}
	if status.code() == Some(1) {
		eyre::bail!(
			"Configured repo root `{}` is on `{default_branch}` but does not contain merge commit `{merge_commit}` for `{pr_url}`.",
			repo_root.display()
		);
	}

	eyre::bail!(
		"`git merge-base --is-ancestor {merge_commit} HEAD` failed in `{}` with status `{}`.",
		repo_root.display(),
		status
	);
}

pub(super) fn local_branch_exists(repo_root: &Path, branch_name: &str) -> Result<bool> {
	let ref_name = format!("refs/heads/{branch_name}");
	let status = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--quiet", ref_name.as_str()])
		.status()?;

	if status.success() {
		return Ok(true);
	}
	if status.code() == Some(1) {
		return Ok(false);
	}

	eyre::bail!(
		"`git show-ref --verify --quiet {ref_name}` failed in `{}` with status `{}`.",
		repo_root.display(),
		status
	);
}
