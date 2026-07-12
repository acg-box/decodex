//! Local Decodex control-plane runtime paths and project registry helpers.

mod global_config;
#[cfg_attr(not(test), allow(dead_code))]
mod generation;
mod paths;
mod projects;

#[cfg(test)]
pub(crate) use self::{
	global_config::write_global_fixed_account_selector, paths::decodex_home_dir_from,
};
pub(crate) use self::{
	paths::{
		accounts_path, agent_evidence_dir, decodex_home_dir, global_config_path, log_dir,
		project_config_dir, runtime_db_path,
	},
	projects::{
		register_project_config, registered_config_path_for_cwd,
		registered_config_path_for_project_id,
	},
};
pub(crate) use global_config::global_fixed_account_selector;

use crate::{prelude::Result, state::StateStore};

/// Open the global single-machine runtime database.
pub(crate) fn open_runtime_store() -> Result<StateStore> {
	StateStore::open(runtime_db_path()?)
}

/// Open the global runtime database without preloading all durable rows.
pub(crate) fn open_runtime_store_lazy() -> Result<StateStore> {
	StateStore::open_lazy(runtime_db_path()?)
}

#[cfg(test)] mod tests;
