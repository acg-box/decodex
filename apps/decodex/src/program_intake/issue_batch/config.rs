use std::{
	env,
	path::{Path, PathBuf},
};

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
};

pub(crate) fn resolve_intake_project_config_path(
	config_path: Option<&Path>,
	project_id: Option<&str>,
	state_store: &StateStore,
) -> Result<PathBuf> {
	if let Some(config_path) = config_path {
		return ServiceConfig::resolve_project_config_path(config_path);
	}
	if let Some(project_id) = project_id {
		let Some(project) = state_store
			.list_projects()?
			.into_iter()
			.find(|project| project.service_id() == project_id)
		else {
			eyre::bail!(
				"Decodex project `{project_id}` is not registered; pass --config <PROJECT_DIR>."
			);
		};

		return Ok(project.config_path().to_path_buf());
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)?.ok_or_else(|| {
		eyre::eyre!(
			"Current directory is not registered to a Decodex project; pass --config <PROJECT_DIR> or --project <SERVICE_ID>."
		)
	})
}

pub(crate) fn register_intake_project_config_for_persist(
	state_store: &StateStore,
	config_path: &Path,
	persist: bool,
) -> Result<()> {
	if persist {
		runtime::register_project_config(state_store, config_path, true)?;
	}

	Ok(())
}
