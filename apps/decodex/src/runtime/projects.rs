//! Runtime project registration and lookup helpers.

use std::{
	cmp::Reverse,
	fs,
	path::{Path, PathBuf},
};

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	state::{ProjectRegistration, StateStore},
};

/// Register or refresh one project config in the global runtime DB.
pub(crate) fn register_project_config(
	state_store: &StateStore,
	config_path: &Path,
	enabled: bool,
) -> Result<ProjectRegistration> {
	let config_path = ServiceConfig::resolve_project_config_path(config_path)?;
	let config_path = fs::canonicalize(config_path)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&config_path,
		&config,
		enabled,
		&config_fingerprint(&config_path, config.workflow_path())?,
	);

	state_store.upsert_project(&registration)
}

/// Resolve the registered project config that owns a local working directory.
pub(crate) fn registered_config_path_for_cwd(
	state_store: &StateStore,
	cwd: &Path,
) -> Result<Option<PathBuf>> {
	let cwd = fs::canonicalize(cwd)?;
	let mut matches = Vec::new();

	for project in state_store.list_projects()? {
		let repo_root = fs::canonicalize(project.repo_root())
			.unwrap_or_else(|_| project.repo_root().to_path_buf());
		let worktree_root = fs::canonicalize(project.worktree_root())
			.unwrap_or_else(|_| project.worktree_root().to_path_buf());
		let matched_root = if cwd.starts_with(&worktree_root) {
			Some(worktree_root)
		} else if cwd.starts_with(&repo_root) {
			Some(repo_root)
		} else {
			None
		};

		if let Some(matched_root) = matched_root {
			matches.push((matched_root.components().count(), project));
		}
	}

	matches.sort_by_key(|item| Reverse(item.0));

	let Some((best_score, best_project)) = matches.first() else {
		return Ok(None);
	};
	let ambiguous = matches.iter().skip(1).any(|(score, project)| {
		score == best_score && project.service_id() != best_project.service_id()
	});

	if ambiguous {
		eyre::bail!(
			"Current directory `{}` matches multiple registered Decodex projects; pass the command's `--config <PROJECT_DIR>`.",
			cwd.display()
		);
	}

	Ok(Some(best_project.config_path().to_path_buf()))
}

/// Resolve one registered project config by stable service id.
pub(crate) fn registered_config_path_for_project_id(
	state_store: &StateStore,
	project_id: &str,
) -> Result<PathBuf> {
	let project_id = project_id.trim();

	if project_id.is_empty() {
		eyre::bail!("Decodex project id cannot be empty.");
	}

	let projects = state_store.list_projects()?;

	if let Some(project) = projects.iter().find(|project| project.service_id() == project_id) {
		return Ok(project.config_path().to_path_buf());
	}

	let registered =
		projects.iter().map(ProjectRegistration::service_id).collect::<Vec<_>>().join(", ");

	eyre::bail!(
		"Decodex project `{project_id}` is not registered. Registered projects: {}.",
		if registered.is_empty() { "none" } else { registered.as_str() }
	)
}

pub(super) fn config_fingerprint(config_path: &Path, workflow_path: &Path) -> Result<String> {
	let config_body = fs::read(config_path)?;
	let workflow_body = fs::read(workflow_path)?;
	let mut hash = 0xcbf29ce484222325_u64;

	for byte in config_path
		.to_string_lossy()
		.bytes()
		.chain(config_body)
		.chain(workflow_path.to_string_lossy().bytes())
		.chain(workflow_body)
	{
		hash ^= u64::from(byte);
		hash = hash.wrapping_mul(0x100000001b3);
	}

	Ok(format!("{hash:016x}"))
}
