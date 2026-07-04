use color_eyre::Report;

use crate::orchestrator::{
	self, AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure,
	AppServerPhaseGoalFailure, AppServerTransportFailure, AppServerTurnFailure,
	AppServerZeroEvidenceStartFailure, RepoGateFailure, RepoGateFailureDisposition,
	StalledRunNeedsAttention,
};

pub(crate) fn retry_comment_details(error: &Report) -> (&'static str, String) {
	debug_assert!(
		!orchestrator::run_failure_writeback_disposition(error).requires_terminal_attention(),
		"terminal-attention failures must not be formatted as retry comments"
	);

	if let Some(repo_gate_failure) = error.downcast_ref::<RepoGateFailure>() {
		match repo_gate_failure.disposition() {
			RepoGateFailureDisposition::ContinueRepair
			| RepoGateFailureDisposition::RetryAfterBackoff => {
				return (
					repo_gate_failure.error_class(),
					repo_gate_failure.retry_next_action().to_owned(),
				);
			},
			RepoGateFailureDisposition::NeedsHumanAttention => {},
		}
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerZeroEvidenceStartFailure>() {
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerCapabilityPreflightFailure>()
		&& app_server_failure.is_retryable_timeout()
	{
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerTransportFailure>()
		&& app_server_failure.is_retryable_startup()
	{
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerPhaseGoalFailure>()
		&& app_server_failure.is_terminal_path_missing()
	{
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}
	if let Some(app_server_failure) = error.downcast_ref::<AppServerDynamicToolFailure>() {
		return (app_server_failure.error_class(), app_server_failure.retry_next_action());
	}

	if error.downcast_ref::<StalledRunNeedsAttention>().is_some() {
		return (
			"stalled_run_detected",
			String::from(
				"decodex will retry the stalled lane automatically; inspect the worktree and app-server activity if the retry budget exhausts",
			),
		);
	}

	if let Some(app_server_failure) = error.downcast_ref::<AppServerTurnFailure>()
		&& app_server_failure.is_retryable_capacity_failure()
	{
		return (
			app_server_failure.error_class(),
			app_server_failure.retry_next_action().to_owned(),
		);
	}

	("retryable_execution_failure", String::from("decodex will retry automatically"))
}
