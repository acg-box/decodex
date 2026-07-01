//! Local Decodex control-plane runtime paths and project registry helpers.

mod global_config;
mod paths;
mod projects;

use crate::{prelude::Result, state::StateStore};

pub(crate) use global_config::global_fixed_account_selector;
#[cfg(test)] pub(crate) use global_config::write_global_fixed_account_selector;
pub(crate) use paths::{
	accounts_path, agent_evidence_dir, decodex_home_dir, global_config_path, log_dir,
	project_config_dir, runtime_db_path,
};
pub(crate) use projects::{
	register_project_config, registered_config_path_for_cwd, registered_config_path_for_project_id,
};

#[cfg(test)] pub(crate) use paths::decodex_home_dir_from;

/// Open the global single-machine runtime database.
pub(crate) fn open_runtime_store() -> Result<StateStore> {
	StateStore::open(runtime_db_path()?)
}

/// Open the global runtime database without preloading all durable rows.
pub(crate) fn open_runtime_store_lazy() -> Result<StateStore> {
	StateStore::open_lazy(runtime_db_path()?)
}

#[cfg(test)] mod tests;
