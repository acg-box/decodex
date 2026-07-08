use crate::{
	agent::app_server::{
		dynamic_tools,
		protocol::{
			self, AppServerClient, EffectiveThreadConfig, ThreadResumeRequest,
			ThreadSessionResponse, ThreadStartRequest,
		},
		runtime_types::{
			AppServerRunRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
		},
		server_requests,
		session::validation,
		transport,
	},
	prelude::Result,
	state::StateStore,
};

pub(in crate::agent::app_server) fn start_or_resume_thread_session(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
) -> Result<ThreadSessionResponse> {
	if let Some(resume_thread_id) = request.resume_thread_id.as_deref() {
		return resume_existing_thread_session(client, recorder, request, resume_thread_id);
	}

	start_fresh_thread_session(client, recorder, request)
}

pub(in crate::agent::app_server) fn record_thread_session_start(
	state_store: &StateStore,
	request: &AppServerRunRequest<'_>,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	effective_thread_config: &EffectiveThreadConfig,
) -> Result<()> {
	state_store.update_run_thread(&request.run_id, thread_id)?;
	recorder.set_thread_id(thread_id)?;
	recorder.set_effective_runtime(effective_thread_config)?;

	validation::validate_effective_thread_config(&request.cwd, effective_thread_config)?;

	recorder.mark_activity()
}

pub(in crate::agent::app_server) fn build_thread_start_request(
	request: &AppServerRunRequest<'_>,
) -> Result<ThreadStartRequest> {
	let dynamic_tools = request
		.dynamic_tool_handler
		.map(dynamic_tools::validated_dynamic_tool_specs)
		.transpose()?
		.map(|tool_specs| self::protocol::app_server_dynamic_tool_specs(&tool_specs));

	Ok(ThreadStartRequest {
		cwd: Some(request.cwd.clone()),
		dynamic_tools,
		developer_instructions: Some(request.developer_instructions.clone()),
		ephemeral: request.ephemeral_thread.then_some(true),
		..ThreadStartRequest::default()
	})
}

pub(in crate::agent::app_server) fn build_thread_resume_request(
	resume_thread_id: &str,
	request: &AppServerRunRequest<'_>,
) -> ThreadResumeRequest {
	ThreadResumeRequest {
		thread_id: resume_thread_id.to_owned(),
		cwd: Some(request.cwd.clone()),
		developer_instructions: Some(request.developer_instructions.clone()),
		..ThreadResumeRequest::default()
	}
}

fn start_fresh_thread_session(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
) -> Result<ThreadSessionResponse> {
	let thread_start_request = build_thread_start_request(request)?;

	transport::annotate_transport_failure_phase(
		client.start_thread_with_handler(
			thread_start_request,
			|connection, wire_message, server_request| {
				server_requests::handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::ThreadStart,
						request.dynamic_tool_handler,
						request.codex_account_provider,
						None,
						None,
					),
				)
			},
		),
		RequestWaitPhase::ThreadStart,
	)
}

fn resume_existing_thread_session(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	resume_thread_id: &str,
) -> Result<ThreadSessionResponse> {
	match client.resume_thread_with_handler(
		build_thread_resume_request(resume_thread_id, request),
		|connection, wire_message, server_request| {
			server_requests::handle_server_request_while_waiting(
				connection,
				recorder,
				wire_message,
				server_request,
				RequestDispatchContext::new(
					RequestWaitPhase::ThreadResume,
					request.dynamic_tool_handler,
					request.codex_account_provider,
					Some(resume_thread_id),
					None,
				),
			)
		},
	) {
		Ok(response) => Ok(response),
		Err(error) if validation::thread_resume_error_allows_fallback(&error) => {
			recorder.record(
				"thread/resume/miss",
				&serde_json::json!({
					"requestedThreadId": resume_thread_id,
					"error": error.to_string(),
				})
				.to_string(),
			)?;

			start_fresh_thread_session(client, recorder, request)
		},
		Err(error) =>
			Err(transport::transport_failure_at_phase(error, RequestWaitPhase::ThreadResume)),
	}
}
