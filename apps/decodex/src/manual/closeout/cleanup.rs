use std::{fs, path::Path};

use color_eyre::eyre::WrapErr;

use crate::{
	github,
	manual::{ManualAuthority, ManualLandContext},
	orchestrator,
	prelude::{Result, eyre},
	worktree::{self, WorktreeManager},
};

pub(super) fn manual_land_relative_worktree_path(context: &ManualLandContext) -> String {
	if let Ok(relative_path) = context.worktree_root.strip_prefix(&context.canonical_repo_root) {
		if relative_path.as_os_str().is_empty() {
			return String::from(".");
		}

		return relative_path.display().to_string();
	}
	if let Some(root_name) = context.project_worktree_root.file_name()
		&& let Ok(relative_path) =
			context.worktree_root.strip_prefix(&context.project_worktree_root)
	{
		return Path::new(root_name).join(relative_path).display().to_string();
	}

	context.worktree_root.file_name().map_or_else(
		|| context.worktree_root.display().to_string(),
		|path| path.to_string_lossy().into_owned(),
	)
}

pub(in crate::manual) fn cleanup_manual_land_lane_checkout(
	context: &ManualLandContext,
) -> Result<()> {
	let worktree_manager = WorktreeManager::new(
		context.service_id.as_str(),
		&context.canonical_repo_root,
		&context.project_worktree_root,
	);

	github::delete_pull_request_head_branch_if_present(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.current_branch,
		&context.github_token,
		context.github_command_path.as_deref(),
	)?;
	orchestrator::detach_worktree_head_from_branch_if_checked_out(
		&context.worktree_root,
		&context.current_branch,
	)?;
	orchestrator::delete_local_branch_if_present(
		&context.canonical_repo_root,
		&context.current_branch,
	)?;

	if let Some(workflow) = context.workflow.as_ref() {
		worktree_manager.remove_worktree_path_with_hooks(
			manual_land_cleanup_identifier(&context.authority, &context.current_branch),
			&context.current_branch,
			&context.worktree_root,
			workflow.frontmatter().execution().workspace_hooks(),
		)?;
	} else {
		worktree_manager.remove_worktree_path(&context.worktree_root)?;
	}

	ensure_manual_land_left_no_merged_worktree_cleanup_debt(context)?;

	Ok(())
}

pub(in crate::manual) fn ensure_manual_land_left_no_merged_worktree_cleanup_debt(
	context: &ManualLandContext,
) -> Result<()> {
	let debts = worktree::merged_worktree_cleanup_debts(
		&context.canonical_repo_root,
		&context.project_worktree_root,
		&context.repository.default_branch,
	)?;

	if debts.is_empty() {
		return Ok(());
	}

	let details = debts
		.iter()
		.map(|debt| {
			format!(
				"{} on {} ({})",
				debt.path.display(),
				debt.branch_name,
				if debt.cleanliness.is_dirty() { "dirty" } else { "clean" }
			)
		})
		.collect::<Vec<_>>()
		.join(", ");

	eyre::bail!(
		"`decodex land` completed the merge but post-land worktree cleanup debt remains under `{}`: {details}. Remove or salvage those worktrees before continuing automation.",
		context.project_worktree_root.display()
	);
}

pub(in crate::manual) fn manual_land_cleanup_identifier<'a>(
	authority: &'a ManualAuthority,
	current_branch: &'a str,
) -> &'a str {
	authority.issue_identifier().unwrap_or(current_branch)
}

pub(in crate::manual) fn ensure_manual_land_checkout_is_managed_lane(
	checkout_root: &Path,
	project_worktree_root: &Path,
	issue_identifier: &str,
) -> Result<()> {
	let canonical_checkout = fs::canonicalize(checkout_root).wrap_err_with(|| {
		format!("Failed to canonicalize current lane checkout `{}`.", checkout_root.display())
	})?;
	let canonical_worktree_root = fs::canonicalize(project_worktree_root).wrap_err_with(|| {
		format!(
			"Failed to canonicalize configured worktree root `{}`.",
			project_worktree_root.display()
		)
	})?;

	if canonical_checkout.starts_with(&canonical_worktree_root)
		&& canonical_checkout != canonical_worktree_root
	{
		return Ok(());
	}

	eyre::bail!(
		"`decodex land` for issue `{issue_identifier}` must run from a managed lane under worktree_root `{}` so successful land can clean up the worktree and branch.",
		project_worktree_root.display()
	);
}
