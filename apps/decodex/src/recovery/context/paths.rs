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

pub(in crate::recovery::context) fn resolve_recovery_config_path(
	config_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<PathBuf> {
	if let Some(config_path) = config_path {
		return ServiceConfig::resolve_project_config_path(config_path);
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)?.ok_or_else(|| {
		eyre::eyre!(
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		)
	})
}
