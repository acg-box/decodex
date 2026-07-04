use std::fs;

use color_eyre::eyre::WrapErr;

use crate::{
	default_branch_sync,
	manual::{
		self, ManualLandContext,
		recovery::{git_checks, worktrees},
	},
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
};

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

pub(in crate::manual) fn ensure_already_merged_manual_land_recovery_ready(
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
	git_checks::ensure_repo_root_default_branch_current(
		&context.canonical_repo_root,
		&context.repository.default_branch,
	)?;
	git_checks::ensure_merge_commit_reachable_from_default_branch(
		&context.canonical_repo_root,
		&context.pr_url,
		merge_commit,
		&context.repository.default_branch,
	)?;
	worktrees::ensure_manual_land_recovery_lane_cleanup_complete(context, landing_state)?;
	manual::ensure_manual_land_left_no_merged_worktree_cleanup_debt(context)?;

	Ok(())
}

fn ensure_already_merged_manual_land_recovery_state(
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
