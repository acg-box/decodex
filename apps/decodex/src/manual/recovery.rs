use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use color_eyre::eyre::WrapErr;

use crate::{
	default_branch_sync, github,
	manual::{
		self, MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT, ManualLandContext, ManualLandRecoveryOutcome,
		ManualLandRequest,
	},
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
};

pub(super) fn finalize_already_merged_manual_land_recovery(
	context: &ManualLandContext,
	request: &ManualLandRequest,
) -> Result<Option<ManualLandRecoveryOutcome>> {
	if !request.manual_authority || request.pr_url.is_none() {
		return Ok(None);
	}
	if !current_checkout_is_repo_root_default_branch(context)? {
		return Ok(None);
	}

	let landing_state = github::inspect_pull_request_landing_state(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		context.github_command_path.as_deref(),
	)?;

	if landing_state.state != "MERGED" {
		eyre::bail!(
			"`decodex land --manual-authority --pr` can recover from the repo-root default branch only after the PR is `MERGED`; `{}` is `{}`.",
			context.pr_url,
			landing_state.state
		);
	}

	let merge_commit = github::wait_for_pull_request_merge_commit(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		MANUAL_LAND_MERGE_VISIBILITY_TIMEOUT,
		context.github_command_path.as_deref(),
	)?;

	ensure_already_merged_manual_land_recovery_ready(context, &landing_state, &merge_commit)?;

	Ok(Some(ManualLandRecoveryOutcome { merge_commit }))
}

pub(super) fn current_checkout_is_repo_root_default_branch(
	context: &ManualLandContext,
) -> Result<bool> {
	let canonical_checkout = fs::canonicalize(&context.worktree_root).wrap_err_with(|| {
		format!("Failed to canonicalize current checkout `{}`.", context.worktree_root.display())
	})?;
	let canonical_repo_root =
		fs::canonicalize(&context.canonical_repo_root).wrap_err_with(|| {
			format!(
				"Failed to canonicalize configured repo root `{}`.",
				context.canonical_repo_root.display()
			)
		})?;

	Ok(canonical_checkout == canonical_repo_root
		&& context.current_branch == context.repository.default_branch)
}

pub(super) fn ensure_already_merged_manual_land_recovery_ready(
	context: &ManualLandContext,
	landing_state: &PullRequestLandingState,
	merge_commit: &str,
) -> Result<()> {
	ensure_already_merged_manual_land_recovery_state(context, landing_state)?;

	default_branch_sync::preflight_repo_root_default_branch_sync(
		&context.canonical_repo_root,
		&context.repository.default_branch,
		Some(context.default_branch_git_credentials()),
	)?;

	ensure_repo_root_default_branch_current(
		&context.canonical_repo_root,
		&context.repository.default_branch,
	)?;
	ensure_merge_commit_reachable_from_default_branch(
		&context.canonical_repo_root,
		&context.pr_url,
		merge_commit,
		&context.repository.default_branch,
	)?;
	ensure_manual_land_recovery_lane_cleanup_complete(context, landing_state)?;

	manual::ensure_manual_land_left_no_merged_worktree_cleanup_debt(context)?;

	Ok(())
}

pub(super) fn ensure_already_merged_manual_land_recovery_state(
	context: &ManualLandContext,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if landing_state.base_ref_name != context.repository.default_branch {
		eyre::bail!(
			"Pull request `{}` targets base branch `{}`, but manual land recovery only accepts already-merged PRs into `{}`.",
			context.pr_url,
			landing_state.base_ref_name,
			context.repository.default_branch
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; manual land recovery only accepts already-merged PRs.",
			context.pr_url,
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Pull request `{}` does not expose the landed head branch required to verify lane cleanup.",
			context.pr_url
		);
	}
	if landing_state.head_ref_name == context.repository.default_branch {
		eyre::bail!(
			"Pull request `{}` uses the default branch `{}` as its head; manual land recovery cannot prove lane cleanup safely.",
			context.pr_url,
			context.repository.default_branch
		);
	}

	Ok(())
}

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

pub(super) fn ensure_manual_land_recovery_lane_cleanup_complete(
	context: &ManualLandContext,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	let pr_head_branch = landing_state.head_ref_name.as_str();

	if local_branch_exists(&context.canonical_repo_root, pr_head_branch)? {
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

pub(super) fn linked_worktree_paths_for_landed_head_under_root(
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

pub(super) fn push_matching_worktree_path(
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

pub(super) fn checkout_path_is_under_worktree_root(
	path: &Path,
	worktree_root: &Path,
) -> Result<bool> {
	if !path.exists() || !worktree_root.exists() {
		return Ok(false);
	}

	let canonical_path = fs::canonicalize(path)?;
	let canonical_root = fs::canonicalize(worktree_root)?;

	Ok(canonical_path.starts_with(&canonical_root) && canonical_path != canonical_root)
}
