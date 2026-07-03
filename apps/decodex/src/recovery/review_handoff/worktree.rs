use std::{
	fs,
	path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{
		context::RecoveryContext,
		git_worktree::{self},
		pull_request_inspection,
	},
	state::WorktreeMapping,
	tracker::TrackerIssue,
};

pub(super) fn validate_rebind_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	validate_retained_pr_worktree(worktree, landing_state, "rebind")
}

pub(super) fn validate_adopt_current_worktree(
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

	if !canonical_worktree.starts_with(&canonical_root) || canonical_worktree == canonical_root {
		eyre::bail!(
			"Manual takeover adopt for issue `{}` must run from a managed lane under worktree_root `{}`.",
			issue.identifier,
			context.config.worktree_root().display()
		);
	}

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

	if local_branch != landing_state.head_ref_name {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but current worktree branch is `{local_branch}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.head_ref_name
		);
	}
	if !git_worktree::worktree_is_clean(&canonical_worktree)? {
		eyre::bail!(
			"Manual takeover worktree `{}` has local changes; adopt requires a clean lane checkout.",
			canonical_worktree.display()
		);
	}

	let local_head = git_worktree::worktree_head_oid(&canonical_worktree)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree has no readable HEAD."))?;

	if landing_state.head_ref_oid != local_head {
		eyre::bail!(
			"Pull request `{}` points at head `{}`, but current worktree HEAD is `{local_head}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.head_ref_oid
		);
	}

	Ok(canonical_worktree)
}

pub(super) fn validate_adopt_absent_handoff_marker(
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

pub(in crate::recovery) fn validate_retained_pr_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
	action_label: &str,
) -> Result<String> {
	let local_branch = git_worktree::worktree_checkout_branch_name(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree is detached."))?;

	if local_branch != worktree.branch_name() {
		eyre::bail!(
			"Retained worktree branch is `{local_branch}`, but runtime mapping expects `{}`.",
			worktree.branch_name()
		);
	}
	if landing_state.head_ref_name != worktree.branch_name() {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but retained lane branch is `{}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.head_ref_name,
			worktree.branch_name()
		);
	}
	if !git_worktree::worktree_is_clean(worktree.worktree_path())? {
		eyre::bail!(
			"Retained worktree `{}` has local changes; {action_label} requires a clean lane checkout.",
			worktree.worktree_path().display(),
		);
	}

	let local_head = git_worktree::worktree_head_oid(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree has no readable HEAD."))?;

	if landing_state.head_ref_oid != local_head {
		eyre::bail!(
			"Pull request `{}` points at head `{}`, but retained worktree HEAD is `{local_head}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.head_ref_oid
		);
	}

	Ok(local_head)
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

pub(in crate::recovery) fn relative_worktree_path_for_recovery(
	context: &RecoveryContext,
	worktree_path: &Path,
) -> Option<String> {
	git_worktree::repository_relative_path(context.config.repo_root(), worktree_path).or_else(
		|| {
			worktree_path
				.strip_prefix(context.config.repo_root())
				.ok()
				.map(|relative| relative.to_string_lossy().to_string())
		},
	)
}
