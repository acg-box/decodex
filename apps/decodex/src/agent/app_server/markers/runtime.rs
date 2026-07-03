use std::path::Path;

use crate::{
	agent::app_server::{EffectiveRuntimeMarker, EffectiveThreadConfig},
	state,
};

pub(in crate::agent::app_server) fn write_effective_runtime_marker_best_effort(
	marker_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	runtime: &EffectiveThreadConfig,
) {
	if let Err(error) = state::write_run_effective_runtime_marker(
		marker_path,
		run_id,
		attempt_number,
		&EffectiveRuntimeMarker {
			thread_id,
			turn_id,
			effective_model: &runtime.model,
			effective_model_provider: &runtime.model_provider,
			effective_cwd: &runtime.cwd,
			effective_approval_policy: &runtime.approval_policy,
			effective_approvals_reviewer: &runtime.approvals_reviewer,
			effective_sandbox_mode: &runtime.sandbox_mode,
		},
	) {
		tracing::warn!(
			?error,
			run_id,
			attempt_number,
			marker_path = %marker_path.display(),
			"Failed to update worktree effective-runtime marker."
		);
	}
}
