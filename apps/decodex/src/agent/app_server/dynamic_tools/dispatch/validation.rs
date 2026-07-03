use crate::agent::app_server::{
	DynamicToolCallParams, DynamicToolCallResponse, JsonRpcRequest, dynamic_tools::dispatch::model,
	serde_json, tracker_tool_bridge,
};

pub(in crate::agent::app_server::dynamic_tools) fn validated_dynamic_tool_call_payload(
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: Option<&str>,
) -> std::result::Result<DynamicToolCallParams, Box<model::DynamicToolCallDispatch>> {
	let payload = serde_json::from_value::<DynamicToolCallParams>(request.params.clone()).map_err(
		|error| {
			Box::new(model::DynamicToolCallDispatch::protocol_failure(
				None,
				None,
				format!("Invalid `item/tool/call` payload: {error}"),
			))
		},
	)?;

	if payload.call_id.trim().is_empty() {
		return Err(Box::new(model::DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from("Dynamic tool call payload included an empty `callId`."),
		)));
	}
	if !tracker_tool_bridge::dynamic_tool_identifier_is_valid(&payload.tool) {
		return Err(Box::new(model::DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from(
				"Dynamic tool call payload included a tool name outside the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
			),
		)));
	}

	if let Some(namespace) = payload.namespace.as_deref()
		&& !tracker_tool_bridge::dynamic_tool_identifier_is_valid(namespace)
	{
		return Err(Box::new(model::DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			String::from(
				"Dynamic tool call payload included a namespace outside the Codex app-server identifier pattern `^[a-zA-Z0-9_-]+$`.",
			),
		)));
	}

	if payload.thread_id != target_thread_id {
		return Err(Box::new(model::DynamicToolCallDispatch::protocol_failure(
			Some(payload.tool),
			payload.namespace,
			format!(
				"Dynamic tool call targeted thread `{}`, but the active thread is `{target_thread_id}`.",
				payload.thread_id
			),
		)));
	}

	if let Some(target_turn_id) = target_turn_id
		&& payload.turn_id != target_turn_id
	{
		tracing::warn!(
			target_thread_id,
			target_turn_id,
			payload_thread_id = payload.thread_id.as_str(),
			payload_turn_id = payload.turn_id.as_str(),
			tool = payload.tool.as_str(),
			namespace = payload.namespace.as_deref().unwrap_or(""),
			"Dynamic tool call turn id differed from the active turn; accepting thread-bound request."
		);
	}

	Ok(payload)
}

pub(in crate::agent::app_server::dynamic_tools) fn validate_dynamic_tool_call_response(
	response: &DynamicToolCallResponse,
) -> Result<(), String> {
	if response.content_items.is_empty() {
		return Err(String::from(
			"Dynamic tool handler returned an invalid response with no `contentItems`.",
		));
	}

	Ok(())
}
