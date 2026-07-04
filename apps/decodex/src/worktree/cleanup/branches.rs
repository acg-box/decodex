use std::{path::Path, process::Command};

use crate::{
	prelude::{Result, eyre},
	state,
	worktree::git,
};

pub(crate) fn infer_default_branch_name(repo_root: &Path) -> Result<Option<String>> {
	if let Some(remote_head) = symbolic_ref(repo_root, "refs/remotes/origin/HEAD")?
		&& let Some(branch_name) = remote_head.strip_prefix("origin/")
		&& !branch_name.is_empty()
	{
		return Ok(Some(branch_name.to_owned()));
	}

	current_branch_name(repo_root)
}

pub(crate) fn branch_merged_into_default(
	repo_root: &Path,
	branch_name: &str,
	default_branch: &str,
) -> Result<Option<()>> {
	let branch_ref = format!("refs/heads/{branch_name}");
	let default_ref = format!("refs/heads/{default_branch}");

	if !git_ref_exists(repo_root, &branch_ref)? || !git_ref_exists(repo_root, &default_ref)? {
		return Ok(None);
	}
	if git_refs_point_to_same_tip(repo_root, &branch_ref, &default_ref)? {
		return Ok(None);
	}
	if branch_tip_is_on_default_first_parent(repo_root, &branch_ref, &default_ref)? {
		return Ok(None);
	}

	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["merge-base", "--is-ancestor", branch_ref.as_str(), default_ref.as_str()])
		.output()?;

	if output.status.success() {
		return Ok(Some(()));
	}
	if output.status.code() == Some(1) {
		return Ok(None);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to determine whether worktree branch `{branch_name}` is merged into `{default_branch}` in `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

pub(crate) fn worktree_cleanliness(
	worktree_path: &Path,
) -> Result<crate::worktree::cleanup::MergedWorktreeCleanliness> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain"])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect worktree cleanliness in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let status = String::from_utf8_lossy(&output.stdout);

	if status
		.lines()
		.filter(|line| !line.trim_end().is_empty())
		.any(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line))
	{
		return Ok(crate::worktree::cleanup::MergedWorktreeCleanliness::Dirty);
	}

	Ok(crate::worktree::cleanup::MergedWorktreeCleanliness::Clean)
}

fn branch_tip_is_on_default_first_parent(
	repo_root: &Path,
	branch_ref: &str,
	default_ref: &str,
) -> Result<bool> {
	let branch_tip = git::git_stdout(repo_root, ["rev-parse", branch_ref], "resolve branch tip")?;
	let first_parent_history = git::git_stdout(
		repo_root,
		["rev-list", "--first-parent", default_ref],
		"list default branch first-parent history",
	)?;

	Ok(first_parent_history.lines().any(|commit| commit == branch_tip))
}

fn git_refs_point_to_same_tip(repo_root: &Path, left_ref: &str, right_ref: &str) -> Result<bool> {
	let left_tip = git::git_stdout(repo_root, ["rev-parse", left_ref], "resolve git ref tip")?;
	let right_tip = git::git_stdout(repo_root, ["rev-parse", right_ref], "resolve git ref tip")?;

	Ok(left_tip == right_tip)
}

fn git_ref_exists(repo_root: &Path, ref_name: &str) -> Result<bool> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--quiet", ref_name])
		.output()?;

	if output.status.success() {
		return Ok(true);
	}
	if output.status.code() == Some(1) {
		return Ok(false);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect git ref `{ref_name}` in `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

fn symbolic_ref(repo_root: &Path, ref_name: &str) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["symbolic-ref", "--quiet", "--short", ref_name])
		.output()?;

	if !output.status.success() {
		return Ok(None);
	}

	let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	Ok((!value.is_empty()).then_some(value))
}

fn current_branch_name(repo_root: &Path) -> Result<Option<String>> {
	let output =
		Command::new("git").arg("-C").arg(repo_root).args(["branch", "--show-current"]).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect current branch in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	Ok((!value.is_empty()).then_some(value))
}
