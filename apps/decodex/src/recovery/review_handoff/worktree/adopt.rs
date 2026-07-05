use std::{
	fs,
	path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{context::RecoveryContext, git_worktree, pull_request_inspection},
	state::WorktreeMapping,
	tracker::TrackerIssue,
};

pub(in crate::recovery) fn validate_adopt_current_worktree(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	landing_state: &PullRequestLandingState,
	cwd: &Path,
	existing_worktree_mapping: Option<&WorktreeMapping>,
) -> Result<PathBuf> {
	let worktree_path = git_worktree::git_toplevel_path(cwd)?;
	let canonical_worktree = fs::canonicalize(&worktree_path).wrap_err_with(|| {
		format!("Failed to canonicalize current worktree `{}`.", worktree_path.display())
	})?;
	let canonical_root = fs::canonicalize(context.config.worktree_root()).wrap_err_with(|| {
		format!(
			"Failed to canonicalize configured worktree root `{}`.",
			context.config.worktree_root().display()
		)
	})?;

	validate_worktree_inside_managed_root(context, issue, &canonical_worktree, &canonical_root)?;

	let local_branch = git_worktree::worktree_checkout_branch_name(&canonical_worktree)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree is detached."))?;

	if let Some(mapping) = existing_worktree_mapping {
		validate_adopt_existing_worktree_mapping(
			context.config.service_id(),
			issue,
			mapping,
			&canonical_worktree,
		)?;
	}

	validate_adopt_branch_matches_pr(&local_branch, landing_state)?;
	validate_adopt_worktree_is_clean(&canonical_worktree)?;
	validate_adopt_head_matches_pr(&canonical_worktree, landing_state)?;

	Ok(canonical_worktree)
}

pub(in crate::recovery) fn validate_adopt_absent_handoff_marker(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	branch_name: &str,
	existing_worktree_mapping: Option<&WorktreeMapping>,
) -> Result<()> {
	let mut branches = vec![branch_name.to_owned()];

	if let Some(mapping) = existing_worktree_mapping
		&& mapping.branch_name() != branch_name
	{
		branches.push(mapping.branch_name().to_owned());
	}

	for branch in branches {
		if context
			.state_store
			.review_handoff_marker(context.config.service_id(), &issue.id, &branch)?
			.is_some()
		{
			eyre::bail!(
				"Issue `{}` already has a retained review lifecycle record for branch `{branch}`; use `decodex land` or `decodex recover review-handoff rebind` instead.",
				issue.identifier
			);
		}
	}

	Ok(())
}

pub(in crate::recovery) fn validate_adopt_existing_worktree_mapping(
	service_id: &str,
	issue: &TrackerIssue,
	mapping: &WorktreeMapping,
	canonical_worktree: &Path,
) -> Result<()> {
	if mapping.project_id() != service_id {
		eyre::bail!(
			"Issue `{}` already has a retained worktree mapping for project `{}`, not `{}`.",
			issue.identifier,
			mapping.project_id(),
			service_id
		);
	}

	let canonical_mapping = fs::canonicalize(mapping.worktree_path()).wrap_err_with(|| {
		format!(
			"Failed to canonicalize retained worktree mapping `{}` for issue `{}`.",
			mapping.worktree_path().display(),
			issue.identifier
		)
	})?;

	if canonical_mapping != canonical_worktree {
		eyre::bail!(
			"Issue `{}` already has a retained worktree mapping at `{}`, but manual takeover adopt is running from `{}`.",
			issue.identifier,
			mapping.worktree_path().display(),
			canonical_worktree.display()
		);
	}

	Ok(())
}

fn validate_worktree_inside_managed_root(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	canonical_worktree: &Path,
	canonical_root: &Path,
) -> Result<()> {
	if canonical_worktree.starts_with(canonical_root) && canonical_worktree != canonical_root {
		return Ok(());
	}

	eyre::bail!(
		"Manual takeover adopt for issue `{}` must run from a managed lane under worktree_root `{}`.",
		issue.identifier,
		context.config.worktree_root().display()
	);
}

fn validate_adopt_branch_matches_pr(
	local_branch: &str,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if local_branch == landing_state.head_ref_name {
		return Ok(());
	}

	eyre::bail!(
		"Pull request `{}` points at branch `{}`, but current worktree branch is `{local_branch}`.",
		pull_request_inspection::landing_url(landing_state),
		landing_state.head_ref_name
	);
}

fn validate_adopt_worktree_is_clean(canonical_worktree: &Path) -> Result<()> {
	if git_worktree::worktree_is_clean(canonical_worktree)? {
		return Ok(());
	}

	eyre::bail!(
		"Manual takeover worktree `{}` has local changes; adopt requires a clean lane checkout.",
		canonical_worktree.display()
	);
}

fn validate_adopt_head_matches_pr(
	canonical_worktree: &Path,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	let local_head = git_worktree::worktree_head_oid(canonical_worktree)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree has no readable HEAD."))?;

	if landing_state.head_ref_oid == local_head {
		return Ok(());
	}

	eyre::bail!(
		"Pull request `{}` points at head `{}`, but current worktree HEAD is `{local_head}`.",
		pull_request_inspection::landing_url(landing_state),
		landing_state.head_ref_oid
	);
}
