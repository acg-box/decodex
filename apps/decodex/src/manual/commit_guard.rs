use std::path::{Path, PathBuf};

use crate::{
	active_run_env::ActiveRunCommitContext,
	config::{self, ServiceConfig},
	manual::{self, ManualAuthority, ManualCommitActiveLaneBlocker},
	prelude::{Result, eyre},
	runtime,
	state::{StateStore, WorktreeMapping},
};

pub(super) fn ensure_manual_commit_not_claimed_by_active_lane(
	config_path: Option<&Path>,
	cwd: &Path,
	worktree_root: &Path,
	requested_authority: &ManualAuthority,
) -> Result<()> {
	let Some(blocker) = manual_commit_active_lane_blocker_from_runtime(
		config_path,
		cwd,
		worktree_root,
		requested_authority,
	)?
	else {
		return Ok(());
	};

	eyre::bail!(
		"`decodex commit` refused to write inside active Decodex-owned lane worktree `{}` on branch `{}` for issue `{}` because the issue has a live runtime claim. Wait for the lane to finish, steer or interrupt the owning run, or clear retained ownership before using the manual commit helper.",
		blocker.worktree_path.display(),
		blocker.branch_name,
		blocker.issue_id,
	)
}

pub(super) fn manual_commit_active_lane_blocker_from_runtime(
	config_path: Option<&Path>,
	cwd: &Path,
	worktree_root: &Path,
	requested_authority: &ManualAuthority,
) -> Result<Option<ManualCommitActiveLaneBlocker>> {
	let state_store = match runtime::open_runtime_store() {
		Ok(state_store) => state_store,
		Err(_error) if config_path.is_none() => return Ok(None),
		Err(error) => return Err(error),
	};
	let Some(config_path) = manual_commit_project_config_path(config_path, cwd, &state_store)?
	else {
		return Ok(None);
	};
	let config = ServiceConfig::from_path(&config_path)?;

	if !manual_commit_checkout_matches_project(worktree_root, &config)? {
		return Ok(None);
	}

	let current_branch = manual::current_branch_name_if_attached(cwd)?;

	manual_commit_active_lane_blocker(
		&state_store,
		config.service_id(),
		worktree_root,
		current_branch.as_deref(),
		requested_authority,
	)
}

pub(super) fn manual_commit_project_config_path(
	config_path: Option<&Path>,
	cwd: &Path,
	state_store: &StateStore,
) -> Result<Option<PathBuf>> {
	if let Some(config_path) = config_path {
		return Ok(Some(ServiceConfig::resolve_project_config_path(config_path)?));
	}

	runtime::registered_config_path_for_cwd(state_store, cwd)
}

pub(super) fn manual_commit_checkout_matches_project(
	worktree_root: &Path,
	config: &ServiceConfig,
) -> Result<bool> {
	Ok(worktree_root == config.repo_root()
		|| config::checkouts_share_repository(worktree_root, config.repo_root())?)
}

pub(super) fn manual_commit_active_lane_blocker(
	state_store: &StateStore,
	service_id: &str,
	worktree_root: &Path,
	current_branch: Option<&str>,
	requested_authority: &ManualAuthority,
) -> Result<Option<ManualCommitActiveLaneBlocker>> {
	for mapping in state_store.list_worktrees(service_id)? {
		if !manual_commit_matches_worktree_mapping(&mapping, worktree_root, current_branch) {
			continue;
		}
		if !state_store.issue_has_active_shared_claim(service_id, mapping.issue_id())? {
			continue;
		}
		if active_run_commit_context_allows_claimed_lane_commit(
			&state_store,
			service_id,
			mapping.issue_id(),
			requested_authority,
		)? {
			continue;
		}

		return Ok(Some(ManualCommitActiveLaneBlocker {
			issue_id: mapping.issue_id().to_owned(),
			branch_name: mapping.branch_name().to_owned(),
			worktree_path: mapping.worktree_path().to_path_buf(),
		}));
	}

	Ok(None)
}

pub(super) fn manual_commit_matches_worktree_mapping(
	mapping: &WorktreeMapping,
	worktree_root: &Path,
	current_branch: Option<&str>,
) -> bool {
	manual::paths_match_for_manual_commit_guard(worktree_root, mapping.worktree_path())
		&& current_branch.is_none_or(|branch| branch == mapping.branch_name())
}

fn active_run_commit_context_allows_claimed_lane_commit(
	state_store: &StateStore,
	service_id: &str,
	issue_id: &str,
	requested_authority: &ManualAuthority,
) -> Result<bool> {
	let Some(context) = ActiveRunCommitContext::from_process_env() else {
		return Ok(false);
	};
	if context.service_id() != service_id || context.issue_id() != issue_id {
		return Ok(false);
	}
	if !requested_authority_matches_active_lane_issue(
		requested_authority,
		context.issue_identifier(),
	) {
		return Ok(false);
	}

	let Some(lease) = state_store.lease_for_issue(issue_id)? else {
		return Ok(false);
	};
	if lease.project_id() != service_id || lease.run_id() != context.run_id() {
		return Ok(false);
	}

	let Some(attempt) = state_store.run_attempt(context.run_id())? else {
		return Ok(false);
	};

	Ok(attempt.issue_id() == issue_id && matches!(attempt.status(), "starting" | "running"))
}

fn requested_authority_matches_active_lane_issue(
	requested_authority: &ManualAuthority,
	issue_identifier: &str,
) -> bool {
	matches!(
		requested_authority,
		ManualAuthority::Issue(authority) if authority.eq_ignore_ascii_case(issue_identifier)
	)
}
