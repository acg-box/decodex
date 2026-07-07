use color_eyre::Report;

use crate::{
	orchestrator::{self, OperatorStatusSnapshot, ServiceConfig, StateStore, WorkflowDocument},
	prelude::Result,
	tracker::IssueTracker,
};

pub(crate) fn runtime_recovery_warning(prefix: &str, error: &Report) -> String {
	format!("{prefix}:{}", runtime_recovery_error_class(error))
}

pub(crate) fn runtime_recovery_error_class(error: &Report) -> &'static str {
	let message = error.to_string().to_ascii_lowercase();

	if message.contains("linear") || message.contains("tracker") {
		return "tracker";
	}
	if message.contains("worktree") || message.contains("work tree") {
		return "worktree";
	}
	if message.contains("runtime") || message.contains("sqlite") || message.contains("database") {
		return "runtime_store";
	}

	"unknown"
}

pub(crate) fn build_diagnose_live_snapshot<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	limit: usize,
) -> Result<OperatorStatusSnapshot>
where
	T: IssueTracker,
{
	let mut snapshot_warnings = Vec::new();

	match orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		tracker,
		config,
		workflow,
		state_store,
	) {
		Ok(recovered_state) => {
			orchestrator::hydrate_status_snapshot_state(config, state_store, recovered_state)?
		},
		Err(error) => {
			let warning = runtime_recovery_warning("diagnose_runtime_recovery_unavailable", &error);

			tracing::warn!(
				project_id = config.service_id(),
				recovery_error_class = runtime_recovery_error_class(&error),
				"Skipped runtime recovery for diagnose; sensitive runtime details were withheld."
			);

			snapshot_warnings.push(warning);
		},
	}

	let mut snapshot = match orchestrator::build_live_operator_status_snapshot(
		tracker,
		config,
		workflow,
		state_store,
		limit,
	) {
		Ok(snapshot) => snapshot,
		Err(error) => {
			let _ = error;

			tracing::warn!(
				project_id = config.service_id(),
				"Fell back to local diagnose snapshot; sensitive runtime details were withheld."
			);

			snapshot_warnings.push(String::from("diagnose_live_observer_unavailable"));

			orchestrator::build_operator_status_snapshot(config, state_store, limit)?
		},
	};

	for warning in snapshot_warnings {
		orchestrator::add_operator_snapshot_warning(&mut snapshot, &warning);
	}

	Ok(snapshot)
}
