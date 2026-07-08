mod closeout;
mod unregistered;

#[cfg(test)]
pub(super) use self::{
	closeout::{
		ensure_cli_repo_context, read_manual_land_lifecycle, resolve_manual_config_path,
		resolve_pr_url,
	},
	unregistered::prepare_unregistered_manual_land_context,
};

use std::{
	env,
	path::{Path, PathBuf},
};

use crate::{
	config::{self, ServiceConfig},
	github::{self},
	manual::{self, ManualLandContext, ManualLandRequest, git},
	prelude::Result,
	runtime,
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
	workflow::WorkflowDocument,
};

pub(super) fn prepare_manual_land_context(
	config_path: Option<&Path>,
	request: &ManualLandRequest,
) -> Result<ManualLandContext> {
	let cwd = env::current_dir()?;
	let worktree_root = git::current_worktree_root(&cwd)?;
	let current_branch = manual::current_branch_name(&cwd)?;

	if request.manual_authority && config_path.is_none() {
		return unregistered::prepare_unregistered_manual_land_context(
			cwd,
			worktree_root,
			current_branch,
			request,
		);
	}

	let resolved_config_path = closeout::resolve_manual_config_path(config_path, &cwd)?;

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

	closeout::ensure_cli_repo_context(&cwd, &config, &canonical_repo_root)?;

	let authority = manual::resolve_land_authority(
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
	let prepared_closeout = closeout::prepare_manual_land_closeout(
		&config,
		&canonical_repo_root,
		workflow.clone(),
		&authority,
	)?;
	let lifecycle_record = match prepared_closeout.as_ref() {
		Some(prepared_closeout) => {
			let state_store = runtime::open_runtime_store()?;

			runtime::register_project_config(&state_store, resolved_config_path, true)?;

			closeout::read_manual_land_lifecycle(
				&state_store,
				config.service_id(),
				&prepared_closeout.issue.id,
				&current_branch,
			)?
		},
		None => None,
	};
	let pr_url = closeout::resolve_pr_url(
		request.pr_url.as_deref(),
		lifecycle_record.as_ref(),
		authority.is_manual(),
	)?;
	let review_branch = lifecycle_record
		.as_ref()
		.map(|record| record.branch_name().to_owned())
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
		landing_required_status_contexts: config
			.github()
			.landing_required_status_contexts()
			.to_vec(),
		landing_required_status_creators: config
			.github()
			.landing_required_status_creators()
			.to_vec(),
		repository,
		prepared_closeout,
		review_lifecycle: lifecycle_record,
		pr_url,
		review_branch,
		public_projection_privacy_classifier,
	})
}
