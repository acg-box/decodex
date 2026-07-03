use crate::{
	agent::{
		app_server::{
			protocol::{AppServerClient, InitializeResponse},
			runtime_types::{RequestDispatchContext, RequestWaitPhase, RunRecorder},
			server_requests,
			session::validation,
			transport,
		},
		json_rpc::ResolvedAppServerCodexHomeEnv,
		tracker_tool_bridge::DynamicToolHandler,
	},
	prelude::Result,
};

pub(in crate::agent::app_server) fn initialize_client_for_run(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	dynamic_tool_handler: Option<&dyn DynamicToolHandler>,
	expected_codex_home: &ResolvedAppServerCodexHomeEnv,
) -> Result<InitializeResponse> {
	let response = transport::annotate_transport_failure_phase(
		client.initialize_with_handler(
			dynamic_tool_handler.is_some(),
			|connection, wire_message, server_request| {
				server_requests::handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::Initialize,
						dynamic_tool_handler,
						None,
						None,
						None,
					),
				)
			},
		),
		RequestWaitPhase::Initialize,
	)?;

	validation::validate_initialize_codex_home(expected_codex_home, &response)?;

	Ok(response)
}
