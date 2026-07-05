use crate::orchestrator::execution_failure::{
	AppServerCapabilityPreflightFailure, AppServerTransportFailure,
	AppServerZeroEvidenceStartFailure, RepoGateFailure, Report,
};

pub(in crate::orchestrator::execution_failure::retryable_writeback) fn retryable_failure_happened_before_effective_agent_execution(
	error: &Report,
) -> bool {
	error.downcast_ref::<AppServerZeroEvidenceStartFailure>().is_some()
		|| error
			.downcast_ref::<AppServerCapabilityPreflightFailure>()
			.is_some_and(AppServerCapabilityPreflightFailure::is_retryable_timeout)
		|| error
			.downcast_ref::<AppServerTransportFailure>()
			.is_some_and(AppServerTransportFailure::is_retryable_startup)
}

pub(in crate::orchestrator::execution_failure::retryable_writeback) fn retryable_failure_validation_result(
	error: &Report,
	retry_error_class: &str,
) -> Option<&'static str> {
	if retry_error_class.starts_with("repo_gate_")
		|| error.downcast_ref::<RepoGateFailure>().is_some()
	{
		Some("failed")
	} else {
		None
	}
}
