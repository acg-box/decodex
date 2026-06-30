//! App-server server request routing and non-interactive response handling.

use super::{
	AppServerClient, ChatgptAuthTokensRefreshParams, ChatgptAuthTokensRefreshResponse,
	CommandExecutionApprovalDecision, CommandExecutionRequestApprovalResponse,
	FileChangeApprovalDecision, FileChangeRequestApprovalResponse, JSONRPC_METHOD_NOT_FOUND,
	JsonRpcConnection, JsonRpcMessage, JsonRpcRequest, McpServerElicitationAction,
	McpServerElicitationRequestResponse, PermissionGrantScope, PermissionsRequestApprovalResponse,
	RequestDispatchContext, RequestWaitPhase, RunRecorder, Serialize,
	ThreadStatusChangedNotification, ToolRequestUserInputResponse, WireMessage,
	dispatch_dynamic_tool_call, dynamic_tool_call_unavailable_for_phase, eyre, message_type,
	record_codex_account_failure, redact_identifier, respond_to_dynamic_tool_call_dispatch,
	serde_json, targets_thread, thread_id_from_value, turn_id_from_value,
};
use color_eyre::eyre::Report;

pub(super) fn handle_server_request_while_waiting(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	wire_message: &WireMessage,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	if targets_thread(wire_message, context.target_thread_id) {
		record_wire_message_safely(recorder, wire_message)?;
		record_interactive_request_state(recorder, request)?;
	} else if request.method == "account/chatgptAuthTokens/refresh" {
		record_codex_account_refresh_request(recorder, request)?;
	}

	dispatch_server_request(connection, recorder, request, context)
}

pub(super) fn handle_server_request_during_turn_execution(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	record_server_request_safely(recorder, request)?;
	record_interactive_request_state(recorder, request)?;

	dispatch_server_request(&mut client.connection, recorder, request, context)
}

fn dispatch_server_request(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	match request.method.as_str() {
		"item/tool/call" if context.phase == RequestWaitPhase::TurnExecution =>
			dispatch_dynamic_tool_call(connection, recorder, request, context),
		"account/chatgptAuthTokens/refresh" =>
			dispatch_codex_account_refresh(connection, recorder, request, context),
		"item/tool/call" => respond_to_dynamic_tool_call_dispatch(
			connection,
			recorder,
			request,
			dynamic_tool_call_unavailable_for_phase(context.phase),
		),
		"item/commandExecution/requestApproval" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/commandExecution/requestApproval/response",
			&CommandExecutionRequestApprovalResponse {
				decision: CommandExecutionApprovalDecision::Decline,
			},
		),
		"item/fileChange/requestApproval" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/fileChange/requestApproval/response",
			&FileChangeRequestApprovalResponse { decision: FileChangeApprovalDecision::Decline },
		),
		"item/tool/requestUserInput" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/tool/requestUserInput/response",
			&ToolRequestUserInputResponse::default(),
		),
		"item/permissions/requestApproval" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/permissions/requestApproval/response",
			&PermissionsRequestApprovalResponse {
				permissions: Default::default(),
				scope: PermissionGrantScope::Turn,
			},
		),
		"mcpServer/elicitation/request" => reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"mcpServer/elicitation/request/response",
			&McpServerElicitationRequestResponse {
				action: McpServerElicitationAction::Decline,
				content: None,
				meta: None,
			},
		),
		other =>
			reject_unsupported_server_request(connection, recorder, request, context.phase, other),
	}
}

pub(super) fn record_server_request(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
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

fn record_server_request_safely(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
	if request.method == "account/chatgptAuthTokens/refresh" {
		return record_codex_account_refresh_request(recorder, request);
	}

	record_server_request(recorder, request)
}

fn record_wire_message_safely(
	recorder: &mut RunRecorder<'_>,
	wire_message: &WireMessage,
) -> crate::prelude::Result<()> {
	match &wire_message.message {
		JsonRpcMessage::Request(request)
			if request.method == "account/chatgptAuthTokens/refresh" =>
			record_codex_account_refresh_request(recorder, request),
		_ => recorder.record(message_type(wire_message), &wire_message.raw),
	}
}

fn record_codex_account_refresh_request(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
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

fn dispatch_codex_account_refresh(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> crate::prelude::Result<()> {
	let account_provider = context.codex_account_provider.ok_or_else(|| {
		eyre::eyre!(
			"app_server_protocol_failure: received `account/chatgptAuthTokens/refresh` without a configured Codex account provider."
		)
	})?;
	let params = serde_json::from_value::<ChatgptAuthTokensRefreshParams>(request.params.clone())?;
	let account = match account_provider.refresh_account(params.previous_account_id.as_deref()) {
		Ok(account) => account,
		Err(error) => {
			record_codex_account_failure(
				recorder,
				"account/chatgptAuthTokens/refresh/failed",
				&error,
			);

			return Err(error);
		},
	};
	let response = ChatgptAuthTokensRefreshResponse {
		access_token: account.access_token().to_owned(),
		chatgpt_account_id: account.account_id().to_owned(),
		chatgpt_plan_type: account.plan_type().map(str::to_owned),
	};

	recorder.set_codex_account(account.summary(), account.account_summaries())?;
	connection.respond(&request.id, &response)?;

	recorder.record(
		"account/chatgptAuthTokens/refresh/response",
		&serde_json::json!({
			"type": "chatgptAuthTokens",
			"accountFingerprint": account.summary().account_fingerprint.as_str(),
			"planType": account.summary().plan_type.as_deref(),
			"refreshStatus": account.summary().refresh_status.as_str(),
			"primaryRemainingPercent": account.summary().primary_remaining_percent,
			"secondaryRemainingPercent": account.summary().secondary_remaining_percent,
		})
		.to_string(),
	)
}

fn reject_unsupported_server_request(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	phase: RequestWaitPhase,
	method: &str,
) -> crate::prelude::Result<()> {
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

pub(super) fn record_server_request_response<T>(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	event_type: &str,
	response: &T,
) -> crate::prelude::Result<()>
where
	T: Serialize,
{
	connection.respond(&request.id, response)?;

	recorder.record(event_type, &serde_json::to_string(response)?)
}

fn reject_interactive_server_request<T>(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	phase: RequestWaitPhase,
	event_type: &str,
	response: &T,
) -> crate::prelude::Result<()>
where
	T: Serialize,
{
	record_server_request_response(connection, recorder, request, event_type, response)?;

	Err(noninteractive_interaction_required(request.method.as_str(), phase))
}

fn noninteractive_interaction_required(method: &str, phase: RequestWaitPhase) -> Report {
	eyre::eyre!(
		"noninteractive_interaction_required: server request `{method}` requires interactive handling during {}.",
		phase.label()
	)
}

pub(super) fn record_interactive_request_state(
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
) -> crate::prelude::Result<()> {
	let Some(flag) = interactive_flag_for_request(request.method.as_str()) else {
		return Ok(());
	};

	if let Some(thread_id) = thread_id_from_value(&request.params) {
		recorder.set_thread_id(thread_id)?;
	}
	if let Some(turn_id) = turn_id_from_value(&request.params) {
		recorder.set_turn_id(turn_id)?;
	}

	recorder.set_thread_status("active", &[flag.to_owned()])
}

pub(super) fn interactive_flag_for_request(method: &str) -> Option<&'static str> {
	match method {
		"item/tool/requestUserInput" => Some("waitingOnUserInput"),
		"item/commandExecution/requestApproval"
		| "item/fileChange/requestApproval"
		| "item/permissions/requestApproval"
		| "mcpServer/elicitation/request" => Some("waitingOnApproval"),
		_ => None,
	}
}

pub(super) fn apply_protocol_message_side_effects(
	recorder: &mut RunRecorder<'_>,
	message: &WireMessage,
) -> crate::prelude::Result<()> {
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
