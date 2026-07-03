use color_eyre::Report;

use crate::{
	agent::{app_server::runtime_types::RequestWaitPhase, json_rpc},
	prelude::Result,
};

pub(super) fn annotate_transport_failure_phase<T>(
	result: Result<T>,
	phase: RequestWaitPhase,
) -> Result<T> {
	result.map_err(|error| transport_failure_at_phase(error, phase))
}

pub(super) fn transport_failure_at_phase(error: Report, phase: RequestWaitPhase) -> Report {
	let Some(transport_failure) = error.downcast_ref::<json_rpc::AppServerTransportFailure>()
	else {
		return error;
	};

	Report::new(json_rpc::AppServerTransportFailure::with_phase(
		transport_failure.to_string(),
		phase.label(),
		phase.transport_failure_is_retryable_startup(),
	))
}
