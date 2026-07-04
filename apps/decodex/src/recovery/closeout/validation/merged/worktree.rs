use std::path::Path;

use crate::{
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	recovery::{closeout::validation::merged::issue, context::RecoveryContext, git_worktree},
	state::WorktreeMapping,
	tracker::TrackerIssue,
};

pub(in crate::recovery) fn validate_merged_closeout_worktree_mapping(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree_mapping: Option<&WorktreeMapping>,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if let Some(mapping) = worktree_mapping {
		if mapping.branch_name() != landing_state.head_ref_name {
			eyre::bail!(
				"Issue `{}` retained worktree branch is `{}`, but merged PR head branch is `{}`.",
				issue.identifier,
				mapping.branch_name(),
				landing_state.head_ref_name
			);
		}

		return validate_merged_closeout_worktree_path(mapping.worktree_path(), landing_state);
	}

	let Some(relative_path) = issue::latest_merged_closeout_source_record(context, issue)?
		.and_then(|record| record.worktree_path)
	else {
		return Ok(());
	};
	let worktree_path = context.config.repo_root().join(relative_path);

	validate_merged_closeout_worktree_path(&worktree_path, landing_state)
}

fn validate_merged_closeout_worktree_path(
	worktree_path: &Path,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if !worktree_path.exists() {
		return Ok(());
	}
	if !git_worktree::worktree_is_clean(worktree_path)? {
		eyre::bail!(
			"Retained worktree `{}` still has local changes; merged closeout recovery will not mark it cleanup-complete.",
			worktree_path.display()
		);
	}

	let local_branch =
		git_worktree::worktree_checkout_branch_name(worktree_path)?.ok_or_else(|| {
			eyre::eyre!("Retained worktree `{}` is detached.", worktree_path.display())
		})?;

	if local_branch != landing_state.head_ref_name {
		eyre::bail!(
			"Retained worktree `{}` is on branch `{local_branch}`, but merged PR head branch is `{}`.",
			worktree_path.display(),
			landing_state.head_ref_name
		);
	}

	let local_head = git_worktree::worktree_head_oid(worktree_path)?.ok_or_else(|| {
		eyre::eyre!("Retained worktree `{}` has no readable HEAD.", worktree_path.display())
	})?;

	if local_head != landing_state.head_ref_oid {
		eyre::bail!(
			"Retained worktree `{}` HEAD is `{local_head}`, but merged PR head is `{}`.",
			worktree_path.display(),
			landing_state.head_ref_oid
		);
	}

	Ok(())
}
