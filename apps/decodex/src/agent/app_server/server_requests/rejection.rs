use color_eyre::eyre::Report;

use crate::{
	agent::app_server::{
		JSONRPC_METHOD_NOT_FOUND, JsonRpcConnection, JsonRpcRequest, RequestWaitPhase, RunRecorder,
		Serialize, eyre, serde_json, server_requests::recording,
	},
	prelude::Result,
};

pub(super) fn reject_unsupported_server_request(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	phase: RequestWaitPhase,
	method: &str,
) -> Result<()> {
	let message = format!("unsupported non-interactive server request `{method}`");

	connection.respond_error(&request.id, JSONRPC_METHOD_NOT_FOUND, &message)?;
	recorder.record(
		"json-rpc/error/response",
		&serde_json::json!({
			"code": JSONRPC_METHOD_NOT_FOUND,
			"message": message,
		})
		.to_string(),
	)?;

	eyre::bail!(
		"app_server_protocol_failure: unsupported server request `{method}` while waiting for {}.",
		phase.label()
	);
}

pub(super) fn reject_interactive_server_request<T>(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	phase: RequestWaitPhase,
	event_type: &str,
	response: &T,
) -> Result<()>
where
	T: Serialize,
{
	recording::record_server_request_response(connection, recorder, request, event_type, response)?;

	Err(noninteractive_interaction_required(request.method.as_str(), phase))
}

fn noninteractive_interaction_required(method: &str, phase: RequestWaitPhase) -> Report {
	eyre::eyre!(
		"noninteractive_interaction_required: server request `{method}` requires interactive handling during {}.",
		phase.label()
	)
}
