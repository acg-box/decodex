use color_eyre::eyre::Report;

use crate::{
	agent::app_server::{
		DynamicToolHandler, JsonRpcConnection, JsonRpcRequest, RequestDispatchContext,
		RequestWaitPhase, RunRecorder,
		dynamic_tools::dispatch::{model::DynamicToolCallDispatch, validation},
		eyre, serde_json, server_requests,
	},
	prelude::Result,
};

pub(in crate::agent::app_server) fn dispatch_dynamic_tool_call(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> Result<()> {
	let target_thread_id = context.target_thread_id.ok_or_else(|| {
		eyre::eyre!("app_server_protocol_failure: turn execution request missing thread context")
	})?;
	let dispatch = handle_dynamic_tool_call(
		context.dynamic_tool_handler,
		request,
		target_thread_id,
		context.target_turn_id,
	);

	respond_to_dynamic_tool_call_dispatch(connection, recorder, request, dispatch)
}

pub(in crate::agent::app_server) fn dynamic_tool_call_unavailable_for_phase(
	phase: RequestWaitPhase,
) -> DynamicToolCallDispatch {
	DynamicToolCallDispatch::protocol_failure(
		None,
		None,
		format!("Dynamic tool calls are unavailable while waiting for {}.", phase.label()),
	)
}

pub(in crate::agent::app_server) fn respond_to_dynamic_tool_call_dispatch(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	dispatch: DynamicToolCallDispatch,
) -> Result<()> {
	server_requests::record_server_request_response(
		connection,
		recorder,
		request,
		"item/tool/call/response",
		&dispatch.response,
	)?;

	if let Some(diagnostic) = dispatch.diagnostic.as_ref() {
		tracing::warn!(
			failure_class = diagnostic.failure_class,
			tool = diagnostic.tool.as_deref().unwrap_or("unknown"),
			next_action = diagnostic.next_action,
			message = diagnostic.message,
			"Dynamic tool call failed."
		);

		recorder.record("item/tool/call/failure", &serde_json::to_string(diagnostic)?)?;
	}
	if let Some(terminal_failure) = dispatch.terminal_failure {
		return Err(Report::new(terminal_failure));
	}

	Ok(())
}

pub(in crate::agent::app_server) fn handle_dynamic_tool_call(
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: Option<&str>,
) -> DynamicToolCallDispatch {
	let payload = match validation::validated_dynamic_tool_call_payload(
		request,
		target_thread_id,
		target_turn_id,
	) {
		Ok(payload) => payload,
		Err(dispatch) => return *dispatch,
	};
	let Some(dynamic_tool_handler) = dynamic_tool_handler else {
		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from("Dynamic tool bridge is unavailable for this run attempt."),
		);
	};
	let tool_specs = dynamic_tool_handler.tool_specs();
	let spec_matches_namespace = tool_specs.iter().any(|spec| {
		spec.name == payload.tool && spec.namespace.as_deref() == payload.namespace.as_deref()
	});

	if !spec_matches_namespace {
		let message = match payload.namespace.as_deref() {
			Some(namespace) => format!(
				"Dynamic tool `{}` was called under namespace `{namespace}`, but this run did not declare that tool namespace.",
				payload.tool
			),
			None => {
				format!("Dynamic tool `{}` is not declared for this run attempt.", payload.tool)
			},
		};

		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			message,
		);
	}

	let response = dynamic_tool_handler.handle_call_with_namespace(
		payload.namespace.as_deref(),
		&payload.tool,
		payload.arguments,
	);

	if let Err(message) = validation::validate_dynamic_tool_call_response(&response) {
		return DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			message,
		);
	}

	if !response.success {
		return DynamicToolCallDispatch::tool_failure(
			response,
			Some(payload.tool),
			payload.namespace,
		);
	}

	DynamicToolCallDispatch::success(response)
}
