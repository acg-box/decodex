use std::{
	env, fs,
	path::{Path, PathBuf},
};

use color_eyre::eyre::WrapErr;

use crate::{
	config, github,
	manual::{self, ManualLandContext, ManualLandRequest, context::closeout},
	prelude::{Result, eyre},
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
};

pub(in crate::manual) fn prepare_unregistered_manual_land_context(
	cwd: PathBuf,
	worktree_root: PathBuf,
	current_branch: String,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let authority = manual::resolve_land_authority(
		None,
		request.authority.as_deref(),
		request.manual_authority,
		&worktree_root,
	)?;
	let canonical_repo_root =
		config::canonical_repo_root_for_checkout(&cwd)?.unwrap_or_else(|| worktree_root.clone());
	let (github_token_env_var, github_token) = resolve_unregistered_github_token(&cwd, None)?;
	let repository = github::inspect_repository_context(&canonical_repo_root, &github_token, None)?;
	let pr_url = closeout::resolve_pr_url(request.pr_url.as_deref(), None, authority.is_manual())?;
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
		review_lifecycle: None,
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

	if manual::paths_match_for_manual_commit_guard(worktree_root, canonical_repo_root)
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
