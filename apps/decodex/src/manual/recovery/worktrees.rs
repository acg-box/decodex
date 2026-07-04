use std::{
	fs,
	path::{Path, PathBuf},
};

use crate::{
	manual::{self, ManualLandContext, recovery::git_checks},
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
};

pub(super) fn ensure_manual_land_recovery_lane_cleanup_complete(
	context: &ManualLandContext,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	let pr_head_branch = landing_state.head_ref_name.as_str();

	if git_checks::local_branch_exists(&context.canonical_repo_root, pr_head_branch)? {
		eyre::bail!(
			"Manual land recovery for `{}` requires the landed lane cleanup to be complete, but local branch `{pr_head_branch}` still exists.",
			context.pr_url
		);
	}

	let worktree_paths = linked_worktree_paths_for_landed_head_under_root(
		&context.canonical_repo_root,
		&context.project_worktree_root,
		pr_head_branch,
		&landing_state.head_ref_oid,
	)?;

	if worktree_paths.is_empty() {
		return Ok(());
	}

	let details =
		worktree_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ");

	eyre::bail!(
		"Manual land recovery for `{}` requires the landed lane cleanup to be complete, but branch `{pr_head_branch}` or its head `{}` is still checked out under `{}`: {details}.",
		context.pr_url,
		landing_state.head_ref_oid,
		context.project_worktree_root.display()
	);
}

fn linked_worktree_paths_for_landed_head_under_root(
	repo_root: &Path,
	worktree_root: &Path,
	branch_name: &str,
	head_oid: &str,
) -> Result<Vec<PathBuf>> {
	let output = manual::run_git_capture(repo_root, &["worktree", "list", "--porcelain"])?;
	let mut matches = Vec::new();
	let mut current_path: Option<PathBuf> = None;
	let mut current_head: Option<String> = None;
	let mut current_branch: Option<String> = None;

	for line in output.lines() {
		if line.is_empty() {
			push_matching_worktree_path(
				&mut matches,
				&mut current_path,
				&mut current_head,
				&mut current_branch,
				worktree_root,
				branch_name,
				head_oid,
			)?;

			continue;
		}

		if let Some(path) = line.strip_prefix("worktree ") {
			push_matching_worktree_path(
				&mut matches,
				&mut current_path,
				&mut current_head,
				&mut current_branch,
				worktree_root,
				branch_name,
				head_oid,
			)?;

			current_path = Some(PathBuf::from(path));
		} else if let Some(head) = line.strip_prefix("HEAD ") {
			current_head = Some(head.to_owned());
		} else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
			current_branch = Some(branch.to_owned());
		}
	}

	push_matching_worktree_path(
		&mut matches,
		&mut current_path,
		&mut current_head,
		&mut current_branch,
		worktree_root,
		branch_name,
		head_oid,
	)?;

	Ok(matches)
}

fn push_matching_worktree_path(
	matches: &mut Vec<PathBuf>,
	path: &mut Option<PathBuf>,
	head: &mut Option<String>,
	branch: &mut Option<String>,
	worktree_root: &Path,
	branch_name: &str,
	head_oid: &str,
) -> Result<()> {
	if (branch.as_deref() == Some(branch_name) || head.as_deref() == Some(head_oid))
		&& let Some(path) = path.take()
		&& checkout_path_is_under_worktree_root(&path, worktree_root)?
	{
		matches.push(path);
	}

	*path = None;
	*head = None;
	*branch = None;

	Ok(())
}

fn checkout_path_is_under_worktree_root(path: &Path, worktree_root: &Path) -> Result<bool> {
	if !path.exists() || !worktree_root.exists() {
		return Ok(false);
	}

	let canonical_path = fs::canonicalize(path)?;
	let canonical_root = fs::canonicalize(worktree_root)?;

	Ok(canonical_path.starts_with(&canonical_root) && canonical_path != canonical_root)
}
