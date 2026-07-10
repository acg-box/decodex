use std::{
	env,
	path::{Path, PathBuf},
};

use crate::{config::ServiceConfig, prelude::Result, runtime, state::StateStore};

pub(super) struct McpContext {
	pub(super) config_path: Option<PathBuf>,
	pub(super) project_id: Option<String>,
	pub(super) state_store: Option<StateStore>,
}
impl McpContext {
	pub(super) fn for_process(config_path: Option<&Path>) -> Result<Self> {
		let state_store = runtime::open_runtime_store_lazy().ok();
		let config_path = resolve_context_config_path(config_path, state_store.as_ref())?;
		let config = config_path.as_ref().map(ServiceConfig::from_path).transpose()?;
		let project_id = config.map(|config| config.service_id().to_owned());

		Ok(Self { config_path, project_id, state_store })
	}

	pub(super) fn project_id(&self) -> Option<&str> {
		self.project_id.as_deref()
	}
}

fn resolve_context_config_path(
	explicit_path: Option<&Path>,
	state_store: Option<&StateStore>,
) -> Result<Option<PathBuf>> {
	if let Some(path) = explicit_path {
		return Ok(Some(path.to_path_buf()));
	}

	let Some(state_store) = state_store else {
		return Ok(None);
	};

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)
}
