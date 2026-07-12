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

use std::{fs::File, io::Read as _, path::PathBuf};

use crate::{
	authority_broker,
	lane_authority::InvocationOrigin,
	prelude::Result,
	state::StateStore,
};

/// Open the global single-machine runtime database.
pub(crate) fn open_runtime_store() -> Result<StateStore> {
	open_runtime_store_for_origin(InvocationOrigin::LocalCli)
}

pub(crate) fn open_runtime_store_for_origin(origin: InvocationOrigin) -> Result<StateStore> {
	let root = decodex_home_dir()?;
	let generation = generation::selected_runtime_generation_from(&root)?;
	let invocation = authority_broker::local_process_invocation_identity(origin, generation)?;
	StateStore::open_with_invocation(
		generation::selected_runtime_db_path_from(&root)?,
		invocation,
	)
}

/// Open the global runtime database without preloading all durable rows.
pub(crate) fn open_runtime_store_lazy() -> Result<StateStore> {
	open_runtime_store_lazy_for_origin(InvocationOrigin::LocalCli)
}

pub(crate) fn open_runtime_store_lazy_for_origin(origin: InvocationOrigin) -> Result<StateStore> {
	let root = decodex_home_dir()?;
	let generation = generation::selected_runtime_generation_from(&root)?;
	let invocation = authority_broker::local_process_invocation_identity(origin, generation)?;
	StateStore::open_lazy_with_invocation(
		generation::selected_runtime_db_path_from(&root)?,
		invocation,
	)
}

pub(crate) fn initialize_fresh_runtime_generation(generation: u64) -> Result<PathBuf> {
	let root = decodex_home_dir()?;
	let mut genesis_hash = [0_u8; 32];
	File::open("/dev/urandom")?.read_exact(&mut genesis_hash)?;
	generation::initialize_fresh_runtime_generation_from(&root, generation, &genesis_hash)
}

#[cfg(test)] mod tests;
