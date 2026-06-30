use std::{
	env, fs,
	path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;

use crate::{
	config::{self, ServiceConfig},
	github::{self},
	prelude::{Result, eyre},
	runtime,
	state::{ReviewHandoffMarker, StateStore},
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
	workflow::WorkflowDocument,
};

use super::{
	ManualAuthority, ManualLandContext, ManualLandRequest, PreparedCloseout, current_branch_name,
	current_worktree_root, paths_match_for_manual_commit_guard, prepare_closeout,
	resolve_land_authority,
};

pub(super) fn prepare_manual_land_context(
	config_path: Option<&Path>,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let cwd = env::current_dir()?;
	let worktree_root = current_worktree_root(&cwd)?;
	let current_branch = current_branch_name(&cwd)?;

	if request.manual_authority && config_path.is_none() {
		return prepare_unregistered_manual_land_context(
			cwd,
			worktree_root,
			current_branch,
			request,
		);
	}

	let resolved_config_path = resolve_manual_config_path(config_path, &cwd)?;

	prepare_configured_manual_land_context(
		cwd,
		worktree_root,
		current_branch,
		&resolved_config_path,
		request,
	)
}

pub(super) fn prepare_configured_manual_land_context(
	cwd: PathBuf,
	worktree_root: PathBuf,
	current_branch: String,
	resolved_config_path: &Path,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let config = ServiceConfig::from_path(resolved_config_path)?;
	let canonical_repo_root = config::canonical_repo_root_for_checkout(&cwd)?
		.unwrap_or_else(|| config.repo_root().to_path_buf());

	ensure_cli_repo_context(&cwd, &config, &canonical_repo_root)?;

	let authority = resolve_land_authority(
		Some(resolved_config_path),
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;
	let github_token = config.github().resolve_token()?;
	let github_command_path = config.github().command_path().map(Path::to_path_buf);
	let repository = github::inspect_repository_context(
		&canonical_repo_root,
		&github_token,
		github_command_path.as_deref(),
	)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let public_projection_privacy_classifier =
		ConfiguredPublicProjectionPrivacyClassifier::from_config(config.privacy_classifier())?;
	let prepared_closeout =
		prepare_manual_land_closeout(&config, &canonical_repo_root, workflow.clone(), &authority)?;
	let handoff = match prepared_closeout.as_ref() {
		Some(prepared_closeout) => {
			let state_store = runtime::open_runtime_store()?;

			runtime::register_project_config(&state_store, resolved_config_path, true)?;

			read_manual_land_handoff(
				&state_store,
				config.service_id(),
				&prepared_closeout.issue.id,
				&current_branch,
			)?
		},
		None => None,
	};
	let pr_url =
		resolve_pr_url(request.pr_url.as_deref(), handoff.as_ref(), authority.is_manual())?;
	let review_branch = handoff
		.as_ref()
		.map(|marker| marker.branch_name().to_owned())
		.unwrap_or_else(|| current_branch.clone());

	Ok(ManualLandContext {
		cwd,
		current_branch,
		worktree_root,
		project_worktree_root: config.worktree_root().to_path_buf(),
		canonical_repo_root,
		authority,
		service_id: config.service_id().to_owned(),
		workflow: Some(workflow),
		github_token_env_var: config.github().token_env_var().to_owned(),
		github_token,
		github_command_path,
		repository,
		prepared_closeout,
		review_handoff: handoff,
		pr_url,
		review_branch,
		public_projection_privacy_classifier,
	})
}

pub(super) fn prepare_unregistered_manual_land_context(
	cwd: PathBuf,
	worktree_root: PathBuf,
	current_branch: String,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let authority = resolve_land_authority(
		None,
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;
	let canonical_repo_root =
		config::canonical_repo_root_for_checkout(&cwd)?.unwrap_or_else(|| worktree_root.clone());
	let (github_token_env_var, github_token) = resolve_unregistered_github_token(&cwd, None)?;
	let repository = github::inspect_repository_context(&canonical_repo_root, &github_token, None)?;
	let pr_url = resolve_pr_url(request.pr_url.as_deref(), None, authority.is_manual())?;
	let project_worktree_root =
		infer_unregistered_manual_land_worktree_root(&canonical_repo_root, &worktree_root);

	Ok(ManualLandContext {
		cwd,
		current_branch: current_branch.clone(),
		worktree_root,
		project_worktree_root,
		canonical_repo_root,
		authority,
		service_id: repository.name.clone(),
		workflow: None,
		github_token_env_var,
		github_token,
		github_command_path: None,
		repository,
		prepared_closeout: None,
		review_handoff: None,
		pr_url,
		review_branch: current_branch,
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	})
}

pub(super) fn resolve_unregistered_github_token(
	cwd: &Path,
	gh_command_path: Option<&Path>,
) -> Result<(String, String)> {
	for env_var in ["GH_TOKEN", "GITHUB_TOKEN"] {
		if let Some(token) = nonempty_env_var(env_var) {
			return Ok((env_var.to_owned(), token));
		}
	}

	let mut command = github::gh_command_with_config(gh_command_path);

	command.args(["auth", "token"]);
	command.current_dir(cwd);
	command
		.env("GH_PROMPT_DISABLED", "1")
		.env("GIT_TERMINAL_PROMPT", "0")
		.env("GCM_INTERACTIVE", "never");

	let output = command.output().wrap_err("Failed to run `gh auth token`.")?;

	if output.status.success() {
		let token = String::from_utf8_lossy(&output.stdout).trim().to_owned();

		if !token.is_empty() {
			return Ok((String::from("GH_TOKEN"), token));
		}
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let detail = stderr.trim();

	eyre::bail!(
		"`decodex land --manual-authority --pr` needs GitHub credentials when no Decodex project config is provided. Set `GH_TOKEN`/`GITHUB_TOKEN`, authenticate `gh auth token`, or pass `--config <PROJECT_DIR>`.{}",
		if detail.is_empty() {
			String::new()
		} else {
			format!(" `gh auth token` failed: {detail}")
		}
	);
}

pub(super) fn nonempty_env_var(name: &str) -> Option<String> {
	env::var(name).ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
}

pub(super) fn infer_unregistered_manual_land_worktree_root(
	canonical_repo_root: &Path,
	worktree_root: &Path,
) -> PathBuf {
	let conventional_worktree_root = canonical_repo_root.join(".worktrees");

	if paths_match_for_manual_commit_guard(worktree_root, canonical_repo_root)
		|| paths_match_for_manual_land_root(worktree_root, &conventional_worktree_root)
	{
		return conventional_worktree_root;
	}

	worktree_root.parent().map_or_else(|| worktree_root.to_path_buf(), Path::to_path_buf)
}

pub(super) fn paths_match_for_manual_land_root(path: &Path, root: &Path) -> bool {
	let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
	let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

	path.starts_with(&root) && path != root
}

pub(super) fn prepare_manual_land_closeout(
	config: &ServiceConfig,
	_canonical_repo_root: &Path,
	workflow: WorkflowDocument,
	authority: &ManualAuthority,
) -> Result<Option<PreparedCloseout>> {
	let Some(authority_issue) = authority.issue_identifier() else {
		return Ok(None);
	};

	prepare_closeout(config, workflow, authority_issue).map(Some)
}

pub(super) fn resolve_manual_config_path(explicit: Option<&Path>, cwd: &Path) -> Result<PathBuf> {
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

pub(super) fn ensure_cli_repo_context(
	cwd: &Path,
	config: &ServiceConfig,
	canonical_repo_root: &Path,
) -> Result<()> {
	let worktree_root = current_worktree_root(cwd)?;

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

pub(super) fn resolve_pr_url(
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

pub(super) fn read_manual_land_handoff(
	state_store: &StateStore,
	service_id: &str,
	issue_id: &str,
	current_branch: &str,
) -> Result<Option<ReviewHandoffMarker>> {
	state_store.review_handoff_marker(service_id, issue_id, current_branch)
}
