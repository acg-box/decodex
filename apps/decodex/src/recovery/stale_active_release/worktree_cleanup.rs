use std::path::{Path, PathBuf};

use color_eyre::eyre::WrapErr;

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	recovery::{
		git_worktree, reports::StaleActiveDiagnostic, stale_active_labels, stale_active_worktree,
	},
	state::{StateStore, WorktreeMapping},
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

#[derive(Clone, Debug)]
pub(super) enum StaleActiveWorktreeCleanup {
	None,
	UnmappedPath(PathBuf),
	Mapped(WorktreeMapping),
}

pub(super) fn preflight_stale_active_worktree_cleanup(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	preflight_stale_active_worktree_cleanup_plan(state_store, diagnostic).map(|_| ())
}

pub(super) fn preflight_stale_active_worktree_cleanup_plan(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<StaleActiveWorktreeCleanup> {
	let issue_keys = stale_active_labels::stale_active_diagnostic_issue_keys(diagnostic);
	let Some(mapping) =
		stale_active_worktree::stale_active_worktree_mapping_for_keys(state_store, &issue_keys)?
	else {
		if let Some(worktree_path) = diagnostic.worktree_path.as_deref().map(PathBuf::from)
			&& stale_active_worktree_path_exists_for_cleanup(
				&diagnostic.issue_identifier,
				&worktree_path,
			)? {
			ensure_stale_active_worktree_clean(&diagnostic.issue_identifier, &worktree_path)?;

			return Ok(StaleActiveWorktreeCleanup::UnmappedPath(worktree_path));
		}

		return Ok(StaleActiveWorktreeCleanup::None);
	};

	if stale_active_worktree_path_exists_for_cleanup(
		&diagnostic.issue_identifier,
		mapping.worktree_path(),
	)? {
		ensure_stale_active_worktree_clean(&diagnostic.issue_identifier, mapping.worktree_path())?;

		return Ok(StaleActiveWorktreeCleanup::Mapped(mapping));
	}

	Ok(StaleActiveWorktreeCleanup::None)
}

pub(super) fn cleanup_stale_active_worktree_mapping(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
	cleanup: StaleActiveWorktreeCleanup,
) -> Result<()> {
	match cleanup {
		StaleActiveWorktreeCleanup::None => {},
		StaleActiveWorktreeCleanup::UnmappedPath(worktree_path) => {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);

			worktree_manager.remove_worktree_path(&worktree_path)?;
		},
		StaleActiveWorktreeCleanup::Mapped(mapping) => {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);

			worktree_manager.remove_worktree_path_with_hooks(
				&diagnostic.issue_identifier,
				mapping.branch_name(),
				mapping.worktree_path(),
				workflow.frontmatter().execution().workspace_hooks(),
			)?;
		},
	};

	state_store.clear_worktree_mapping(&diagnostic.issue_id)?;

	if diagnostic.issue_identifier != diagnostic.issue_id {
		state_store.clear_worktree_mapping(&diagnostic.issue_identifier)?;
	}

	Ok(())
}

fn stale_active_worktree_path_exists_for_cleanup(
	issue_identifier: &str,
	worktree_path: &Path,
) -> Result<bool> {
	worktree_path.try_exists().wrap_err_with(|| {
		format!(
			"`recover stale-active release` refused `{}` because retained worktree `{}` could not be inspected before cleanup.",
			issue_identifier,
			worktree_path.display()
		)
	})
}

fn ensure_stale_active_worktree_clean(issue_identifier: &str, worktree_path: &Path) -> Result<()> {
	if git_worktree::worktree_has_tracked_changes_for_recovery(worktree_path)? {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because retained worktree changes appeared before cleanup.",
			issue_identifier
		);
	}

	Ok(())
}
