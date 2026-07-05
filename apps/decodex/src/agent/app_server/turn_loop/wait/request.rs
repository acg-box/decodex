use crate::{
	agent::{
		app_server::{
			AppServerClient, RunRecorder,
			runtime_types::{RequestDispatchContext, RequestWaitPhase},
			server_requests,
		},
		codex_accounts::CodexAccountProvider,
		json_rpc::JsonRpcRequest,
		tracker_tool_bridge::DynamicToolHandler,
	},
	prelude::Result,
};

pub(in crate::agent::app_server::turn_loop::wait) fn handle_turn_execution_request(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &JsonRpcRequest,
	target_thread_id: &str,
	target_turn_id: &str,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	codex_account_provider: Option<&dyn CodexAccountProvider>,
) -> Result<()> {
	server_requests::handle_server_request_during_turn_execution(
		client,
		recorder,
		request,
		RequestDispatchContext::new(
			RequestWaitPhase::TurnExecution,
			dynamic_tool_handler,
			codex_account_provider,
			Some(target_thread_id),
			Some(target_turn_id),
		),
	)
}

pub(in crate::agent::app_server::turn_loop::wait) fn ignore_orphan_turn_json_rpc_response() {
	tracing::debug!(
		"Recorded and ignored orphan app-server JSON-RPC response while waiting for turn completion."
	);
}
