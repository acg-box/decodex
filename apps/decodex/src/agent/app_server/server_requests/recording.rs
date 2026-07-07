use crate::{
	agent::app_server::{
		self, ChatgptAuthTokensRefreshParams, JsonRpcConnection, JsonRpcMessage, JsonRpcRequest,
		RunRecorder, Serialize, ThreadStatusChangedNotification, WireMessage, redact_identifier,
		serde_json,
	},
	prelude::Result,
};

pub(in crate::agent::app_server) fn record_server_request(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> Result<()> {
	recorder.record(
		request.method.as_str(),
		&serde_json::json!({
			"id": request.id.clone(),
			"method": request.method.clone(),
			"params": request.params.clone(),
		})
		.to_string(),
	)
}

pub(in crate::agent::app_server) fn record_server_request_response<T>(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	event_type: &str,
	response: &T,
) -> Result<()>
where
	T: Serialize,
{
	connection.respond(&request.id, response)?;

	recorder.record(event_type, &serde_json::to_string(response)?)
}

pub(in crate::agent::app_server) fn record_interactive_request_state(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> Result<()> {
	let Some(flag) = interactive_flag_for_request(request.method.as_str()) else {
		return Ok(());
	};

	if let Some(thread_id) = app_server::thread_id_from_value(&request.params) {
		recorder.set_thread_id(thread_id)?;
	}
	if let Some(turn_id) = app_server::turn_id_from_value(&request.params) {
		recorder.set_turn_id(turn_id)?;
	}

	recorder.set_thread_status("active", &[flag.to_owned()])
}

pub(in crate::agent::app_server) fn interactive_flag_for_request(
	method: &str,
) -> Option<&'static str> {
	match method {
		"item/tool/requestUserInput" => Some("waitingOnUserInput"),
		"item/commandExecution/requestApproval"
		| "item/fileChange/requestApproval"
		| "item/permissions/requestApproval"
		| "mcpServer/elicitation/request" => Some("waitingOnApproval"),
		_ => None,
	}
}

pub(in crate::agent::app_server) fn apply_protocol_message_side_effects(
	recorder: &mut RunRecorder<'_>,
	message: &WireMessage,
) -> Result<()> {
	match &message.message {
		JsonRpcMessage::Notification(notification)
			if notification.method == "thread/status/changed" =>
		{
			let payload: ThreadStatusChangedNotification =
				serde_json::from_value(notification.params.clone())?;

			if recorder.thread_id.is_none() {
				recorder.set_thread_id(&payload.thread_id)?;
			}

			recorder.set_thread_status(&payload.status.kind, &payload.status.active_flags)?;
		},
		_ => {},
	}

	Ok(())
}

pub(super) fn record_server_request_safely(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> Result<()> {
	if request.method == "account/chatgptAuthTokens/refresh" {
		return record_codex_account_refresh_request(recorder, request);
	}

	record_server_request(recorder, request)
}

pub(super) fn record_wire_message_safely(
	recorder: &mut RunRecorder<'_>,
	wire_message: &WireMessage,
) -> Result<()> {
	match &wire_message.message {
		JsonRpcMessage::Request(request)
			if request.method == "account/chatgptAuthTokens/refresh" =>
		{
			record_codex_account_refresh_request(recorder, request)
		},
		_ => recorder.record(app_server::message_type(wire_message), &wire_message.raw),
	}
}

pub(super) fn record_codex_account_refresh_request(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> Result<()> {
	let params = serde_json::from_value::<ChatgptAuthTokensRefreshParams>(request.params.clone())
		.unwrap_or(ChatgptAuthTokensRefreshParams { reason: None, previous_account_id: None });

	recorder.record(
		"account/chatgptAuthTokens/refresh",
		&serde_json::json!({
			"id": request.id.clone(),
			"method": request.method.as_str(),
			"reason": params.reason.as_deref(),
			"previousAccountFingerprint": params.previous_account_id.as_deref().map(redact_identifier),
		})
		.to_string(),
	)
}
