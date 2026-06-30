use color_eyre::Report;

use super::runtime_types::RequestWaitPhase;
use crate::agent::json_rpc;

pub(super) fn annotate_transport_failure_phase<T>(
	result: crate::prelude::Result<T>,
	phase: RequestWaitPhase,
) -> crate::prelude::Result<T> {
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
