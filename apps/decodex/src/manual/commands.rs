use std::{env, path::Path};

use crate::{
	commit_message, default_branch_sync, github,
	manual::{
		authority, closeout, commit_guard, context, git, landing,
		model::{ManualCommitRequest, ManualLandRequest},
		recovery,
	},
	prelude::{Result, eyre},
};

pub(crate) fn run_commit(config_path: Option<&Path>, request: &ManualCommitRequest) -> Result<()> {
	let cwd = env::current_dir()?;
	let worktree_root = git::current_worktree_root(&cwd)?;
	let authority = authority::resolve_authority(
		config_path,
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;

	commit_guard::ensure_manual_commit_not_claimed_by_active_lane(
		config_path,
		&cwd,
		&worktree_root,
	)?;

	let message = commit_message::build_commit_message(
		&request.summary,
		authority.commit_message_value(),
		&request.related,
		request.breaking,
	)?;

	git::run_git_checked_with_stdio(&cwd, &["commit", "-S", "-m", message.as_str()])
}

pub(crate) fn run_land(config_path: Option<&Path>, request: &ManualLandRequest) -> Result<()> {
	let context = context::prepare_manual_land_context(config_path, request)?;

	if !github::pull_request_matches_repository(&context.pr_url, &context.repository)? {
		eyre::bail!(
			"Pull request `{}` does not belong to the current repository `{}/{}`.",
			context.pr_url,
			context.repository.owner,
			context.repository.name,
		);
	}

	if let Some(recovery) =
		recovery::finalize_already_merged_manual_land_recovery(&context, request)?
	{
		println!(
			"land ok: pr={} merge_commit={} default_branch={} local_default_branch_synced=true",
			context.pr_url, recovery.merge_commit, context.repository.default_branch
		);

		return Ok(());
	}

	closeout::ensure_manual_land_checkout_is_managed_lane(
		&context.worktree_root,
		&context.project_worktree_root,
		closeout::manual_land_cleanup_identifier(&context.authority, &context.current_branch),
	)?;

	if context.current_branch == context.repository.default_branch {
		eyre::bail!("`decodex land` must run from a reviewed lane branch, not the default branch.");
	}
	if context.review_branch != context.current_branch {
		eyre::bail!(
			"Review handoff expects branch `{}`, but the current branch is `{}`.",
			context.review_branch,
			context.current_branch,
		);
	}
	if context.prepared_closeout.is_some() && context.review_lifecycle.is_none() {
		eyre::bail!(
			"`decodex land` issue closeout requires a retained review lifecycle authority so it can write deterministic Linear execution ledger events. Run `decodex recover review-handoff rebind` for `{}` before retrying.",
			context.current_branch
		);
	}

	let default_branch = context.repository.default_branch.clone();
	let landing_state = landing::inspect_pull_request_landing_state_for_manual_land(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.github_token,
		context.github_command_path.as_deref(),
	)?;
	let current_head = git::current_head_oid(&context.cwd)?;
	let execution_mode = landing::validate_landing_state(
		&landing_state,
		&context.pr_url,
		&default_branch,
		&context.current_branch,
		&current_head,
	)?;

	default_branch_sync::preflight_repo_root_default_branch_sync(
		&context.canonical_repo_root,
		&default_branch,
		Some(context.default_branch_git_credentials()),
	)?;

	let landed_change_record = commit_message::build_landing_commit_message(
		&request.summary,
		context.authority.commit_message_value(),
		&request.related,
		request.breaking,
	)?;
	let merge_commit = landing::execute_land_merge(
		&context,
		&current_head,
		landed_change_record.as_str(),
		execution_mode,
	)?;
	let landed_change_record =
		landing::load_authoritative_landed_change_record(&context, &merge_commit)?;

	closeout::finalize_land_closeout(
		&context,
		&merge_commit,
		&default_branch,
		landed_change_record.as_str(),
	)?;

	println!(
		"land ok: pr={} merge_commit={} default_branch={} local_default_branch_synced=true",
		context.pr_url, merge_commit, default_branch
	);

	Ok(())
}
