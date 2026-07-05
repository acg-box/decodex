use std::path::Path;

use crate::{
	config::ServiceConfig,
	prelude::Result,
	recovery::context::{LinearClient, RecoveryContext, RecoveryRuntimeMutationPolicy, paths},
	runtime,
	workflow::WorkflowDocument,
};

pub(in crate::recovery) fn load_recovery_context_read_only(
	config_path: Option<&Path>,
) -> Result<RecoveryContext> {
	load_recovery_context_with_policy(config_path, RecoveryRuntimeMutationPolicy::ReadOnly)
}

pub(in crate::recovery) fn load_recovery_context_for_dry_run(
	config_path: Option<&Path>,
	dry_run: bool,
) -> Result<RecoveryContext> {
	let runtime_mutation_policy = if dry_run {
		RecoveryRuntimeMutationPolicy::ReadOnly
	} else {
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites
	};

	load_recovery_context_with_policy(config_path, runtime_mutation_policy)
}

fn load_recovery_context_with_policy(
	config_path: Option<&Path>,
	runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
) -> Result<RecoveryContext> {
	let state_store = runtime::open_runtime_store()?;
	let config_path = paths::resolve_recovery_config_path(config_path, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	if runtime_mutation_policy.allows_runtime_writes() {
		runtime::register_project_config(&state_store, &config_path, true)?;
	}

	state_store.observe_dispatch_slot_root(config.service_id(), config.worktree_root())?;

	Ok(RecoveryContext { config, workflow, state_store, tracker, runtime_mutation_policy })
}
