use crate::{
	agent::app_server::{
		self, AppServerClient, ChatgptAuthTokensRefreshParams, ChatgptAuthTokensRefreshResponse,
		CommandExecutionApprovalDecision, CommandExecutionRequestApprovalResponse,
		FileChangeApprovalDecision, FileChangeRequestApprovalResponse, JsonRpcConnection,
		JsonRpcRequest, McpServerElicitationAction, McpServerElicitationRequestResponse,
		PermissionGrantScope, PermissionsRequestApprovalResponse, RequestDispatchContext,
		RequestWaitPhase, RunRecorder, ToolRequestUserInputResponse, WireMessage, eyre, serde_json,
		server_requests::{recording, rejection},
	},
	prelude::Result,
};

pub(in crate::agent::app_server) fn handle_server_request_while_waiting(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	wire_message: &WireMessage,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> Result<()> {
	if app_server::targets_thread(wire_message, context.target_thread_id) {
		recording::record_wire_message_safely(recorder, wire_message)?;
		recording::record_interactive_request_state(recorder, request)?;
	} else if request.method == "account/chatgptAuthTokens/refresh" {
		recording::record_codex_account_refresh_request(recorder, request)?;
	}

	dispatch_server_request(connection, recorder, request, context)
}

pub(in crate::agent::app_server) fn handle_server_request_during_turn_execution(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> Result<()> {
	recording::record_server_request_safely(recorder, request)?;
	recording::record_interactive_request_state(recorder, request)?;

	dispatch_server_request(&mut client.connection, recorder, request, context)
}

fn dispatch_server_request(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> Result<()> {
	match request.method.as_str() {
		"item/tool/call" if context.phase == RequestWaitPhase::TurnExecution => {
			app_server::dispatch_dynamic_tool_call(connection, recorder, request, context)
		},
		"account/chatgptAuthTokens/refresh" => {
			dispatch_codex_account_refresh(connection, recorder, request, context)
		},
		"item/tool/call" => app_server::respond_to_dynamic_tool_call_dispatch(
			connection,
			recorder,
			request,
			app_server::dynamic_tool_call_unavailable_for_phase(context.phase),
		),
		"item/commandExecution/requestApproval" => rejection::reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/commandExecution/requestApproval/response",
			&CommandExecutionRequestApprovalResponse {
				decision: CommandExecutionApprovalDecision::Decline,
			},
		),
		"item/fileChange/requestApproval" => rejection::reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/fileChange/requestApproval/response",
			&FileChangeRequestApprovalResponse { decision: FileChangeApprovalDecision::Decline },
		),
		"item/tool/requestUserInput" => rejection::reject_interactive_server_request(
			connection,
			recorder,
			request,
			context.phase,
			"item/tool/requestUserInput/response",
			&ToolRequestUserInputResponse::default(),
		),
		"item/permissions/requestApproval" => rejection::reject_interactive_server_request(
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
		"mcpServer/elicitation/request" => rejection::reject_interactive_server_request(
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
		other => rejection::reject_unsupported_server_request(
			connection,
			recorder,
			request,
			context.phase,
			other,
		),
	}
}

fn dispatch_codex_account_refresh(
	connection: &mut JsonRpcConnection,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	context: RequestDispatchContext<'_>,
) -> Result<()> {
	let account_provider = context.codex_account_provider.ok_or_else(|| {
		eyre::eyre!(
			"app_server_protocol_failure: received `account/chatgptAuthTokens/refresh` without a configured Codex account provider."
		)
	})?;
	let params = serde_json::from_value::<ChatgptAuthTokensRefreshParams>(request.params.clone())?;
	let account = match account_provider.refresh_account(params.previous_account_id.as_deref()) {
		Ok(account) => account,
		Err(error) => {
			app_server::record_codex_account_failure(
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
