use std::path::Path;

use crate::{
	config::ServiceConfig,
	orchestrator,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
};

pub(super) fn load_lane_control_project(
	config_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<ServiceConfig> {
	let Some(config_path) = orchestrator::resolve_config_path(config_path, state_store)? else {
		eyre::bail!(
			"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		);
	};

	runtime::register_project_config(state_store, &config_path, true)?;

	ServiceConfig::from_path(&config_path)
}

pub(super) fn load_lane_control_project_for_optional_id(
	config_path: Option<&Path>,
	project_id: Option<&str>,
	state_store: &StateStore,
) -> Result<ServiceConfig> {
	let Some(project_id) = project_id.map(str::trim).filter(|id| !id.is_empty()) else {
		return load_lane_control_project(config_path, state_store);
	};
	let config_path = if let Some(config_path) = config_path {
		ServiceConfig::resolve_project_config_path(config_path)?
	} else {
		state_store
			.list_projects()?
			.into_iter()
			.find(|registration| registration.service_id() == project_id)
			.map(|registration| registration.config_path().to_path_buf())
			.ok_or_else(|| {
				eyre::eyre!(
					"Decodex project `{project_id}` is not registered. Pass --config or run `decodex project add`."
				)
			})?
	};

	runtime::register_project_config(state_store, &config_path, true)?;

	let project = ServiceConfig::from_path(&config_path)?;

	if project.service_id() != project_id {
		eyre::bail!(
			"Lane steer project `{project_id}` did not match config service id `{}`.",
			project.service_id()
		);
	}

	Ok(project)
}
