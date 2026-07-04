use std::path::{Path, PathBuf};

use crate::{
	config::{self, ServiceConfig},
	manual::{self, ManualAuthority, PreparedCloseout, git},
	prelude::{Result, eyre},
	runtime,
	state::{ReviewHandoffMarker, StateStore},
	workflow::WorkflowDocument,
};

pub(in crate::manual) fn prepare_manual_land_closeout(
	config: &ServiceConfig,
	_canonical_repo_root: &Path,
	workflow: WorkflowDocument,
	authority: &ManualAuthority,
) -> Result<Option<PreparedCloseout>> {
	let Some(authority_issue) = authority.issue_identifier() else {
		return Ok(None);
	};

	manual::prepare_closeout(config, workflow, authority_issue).map(Some)
}

pub(in crate::manual) fn resolve_manual_config_path(
	explicit: Option<&Path>,
	cwd: &Path,
) -> Result<PathBuf> {
	if let Some(explicit) = explicit {
		return Ok(explicit.to_path_buf());
	}

	let state_store = runtime::open_runtime_store()?;

	if let Some(registered) = runtime::registered_config_path_for_cwd(&state_store, cwd)? {
		return Ok(registered);
	}

	eyre::bail!(
		"Decodex project config is required for this command. Pass this command's `--config <PROJECT_DIR>` or register one with `decodex project add <PROJECT_DIR>`."
	);
}

pub(in crate::manual) fn ensure_cli_repo_context(
	cwd: &Path,
	config: &ServiceConfig,
	canonical_repo_root: &Path,
) -> Result<()> {
	let worktree_root = git::current_worktree_root(cwd)?;

	if worktree_root == canonical_repo_root
		|| config::checkouts_share_repository(&worktree_root, canonical_repo_root)?
	{
		let config_repo_root = config.repo_root();

		if config_repo_root == canonical_repo_root
			|| config::checkouts_share_repository(config_repo_root, canonical_repo_root)?
		{
			return Ok(());
		}
	}

	eyre::bail!(
		"Current worktree `{}` does not match loaded config repo root `{}` for canonical repo root `{}`.",
		worktree_root.display(),
		config.repo_root().display(),
		canonical_repo_root.display(),
	);
}

pub(in crate::manual) fn resolve_pr_url(
	explicit: Option<&str>,
	handoff: Option<&ReviewHandoffMarker>,
	manual_authority: bool,
) -> Result<String> {
	if let Some(explicit) = explicit {
		return Ok(explicit.trim().to_owned());
	}
	if let Some(handoff) = handoff {
		return Ok(handoff.pr_url().to_owned());
	}

	if manual_authority {
		eyre::bail!("`decodex land --manual-authority` requires `--pr <URL>`.");
	}

	eyre::bail!(
		"`decodex land` requires a PR URL. Run it from a handoff worktree or pass `--pr <URL>`."
	);
}

pub(in crate::manual) fn read_manual_land_handoff(
	state_store: &StateStore,
	service_id: &str,
	issue_id: &str,
	current_branch: &str,
) -> Result<Option<ReviewHandoffMarker>> {
	state_store.review_handoff_marker(service_id, issue_id, current_branch)
}
