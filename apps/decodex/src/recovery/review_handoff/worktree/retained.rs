use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{git_worktree, pull_request_inspection},
	state::WorktreeMapping,
};

pub(in crate::recovery) fn validate_rebind_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	validate_retained_pr_worktree(worktree, landing_state, "rebind")
}

pub(in crate::recovery) fn validate_retained_pr_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
	action_label: &str,
) -> Result<String> {
	let local_branch = git_worktree::worktree_checkout_branch_name(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree is detached."))?;

	validate_retained_branch_matches_mapping(&local_branch, worktree)?;
	validate_retained_branch_matches_pr(worktree, landing_state)?;
	validate_retained_worktree_is_clean(worktree, action_label)?;

	let local_head = git_worktree::worktree_head_oid(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree has no readable HEAD."))?;

	validate_retained_head_matches_pr(&local_head, landing_state)?;

	Ok(local_head)
}

fn validate_retained_branch_matches_mapping(
	local_branch: &str,
	worktree: &WorktreeMapping,
) -> Result<()> {
	if local_branch == worktree.branch_name() {
		return Ok(());
	}

	eyre::bail!(
		"Retained worktree branch is `{local_branch}`, but runtime mapping expects `{}`.",
		worktree.branch_name()
	);
}

fn validate_retained_branch_matches_pr(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if landing_state.head_ref_name == worktree.branch_name() {
		return Ok(());
	}

	eyre::bail!(
		"Pull request `{}` points at branch `{}`, but retained lane branch is `{}`.",
		pull_request_inspection::landing_url(landing_state),
		landing_state.head_ref_name,
		worktree.branch_name()
	);
}

fn validate_retained_worktree_is_clean(
	worktree: &WorktreeMapping,
	action_label: &str,
) -> Result<()> {
	if git_worktree::worktree_is_clean(worktree.worktree_path())? {
		return Ok(());
	}

	eyre::bail!(
		"Retained worktree `{}` has local changes; {action_label} requires a clean lane checkout.",
		worktree.worktree_path().display(),
	);
}

fn validate_retained_head_matches_pr(
	local_head: &str,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if landing_state.head_ref_oid == local_head {
		return Ok(());
	}

	eyre::bail!(
		"Pull request `{}` points at head `{}`, but retained worktree HEAD is `{local_head}`.",
		pull_request_inspection::landing_url(landing_state),
		landing_state.head_ref_oid
	);
}
