use std::path::{Path, PathBuf};

use color_eyre::Report;

use crate::{
	agent::{
		app_server::{
			dynamic_tools::validated_dynamic_tool_specs,
			protocol::{
				self, AppServerClient, EffectiveThreadConfig, InitializeResponse,
				LoginAccountParams, LoginAccountResponse, ThreadResumeRequest,
				ThreadSessionResponse, ThreadStartRequest,
			},
			runtime_types::{
				AppServerRunRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
			},
			server_requests,
			transport::{self},
		},
		codex_accounts::{CodexAccountAuthFailure, CodexAccountLogin},
		json_rpc::{AppServerHomePreflightFailure, ResolvedAppServerCodexHomeEnv},
		tracker_tool_bridge::DynamicToolHandler,
	},
	prelude::{Result, eyre},
	state::{CodexAccountActivitySummary, StateStore},
};

pub(super) fn initialize_client_for_run(
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

	validate_initialize_codex_home(expected_codex_home, &response)?;

	Ok(response)
}

pub(super) fn login_codex_account_for_run(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
) -> Result<()> {
	let Some(account_provider) = request.codex_account_provider else {
		return Ok(());
	};
	let account = match account_provider.select_account() {
		Ok(account) => account,
		Err(error) => {
			record_codex_account_failure(recorder, "account/login/select/failed", &error);

			return Err(error);
		},
	};

	recorder.set_codex_account(account.summary(), account.account_summaries())?;

	record_codex_account_login(recorder, account.summary())?;

	let response = transport::annotate_transport_failure_phase(
		client.login_account_with_handler(
			login_account_params(&account),
			|connection, wire_message, server_request| {
				server_requests::handle_server_request_while_waiting(
					connection,
					recorder,
					wire_message,
					server_request,
					RequestDispatchContext::new(
						RequestWaitPhase::AccountLogin,
						request.dynamic_tool_handler,
						request.codex_account_provider,
						None,
						None,
					),
				)
			},
		),
		RequestWaitPhase::AccountLogin,
	)?;

	match response {
		LoginAccountResponse::ChatgptAuthTokens {} => {
			recorder.record(
				"account/login/start/response",
				&serde_json::json!({
					"type": "chatgptAuthTokens",
					"accountFingerprint": account.summary().account_fingerprint.as_str(),
					"planType": account.summary().plan_type.as_deref(),
				})
				.to_string(),
			)?;
		},
	}

	Ok(())
}

pub(super) fn record_codex_account_failure(
	recorder: &mut RunRecorder<'_>,
	event_type: &str,
	error: &Report,
) {
	let auth_failure = error.downcast_ref::<CodexAccountAuthFailure>();
	let error_class =
		auth_failure.map(CodexAccountAuthFailure::error_class).unwrap_or("codex_account_failure");
	let account_fingerprint = auth_failure.and_then(CodexAccountAuthFailure::account_fingerprint);
	let email = auth_failure.and_then(CodexAccountAuthFailure::email);
	let reason =
		auth_failure.map_or_else(|| error.to_string(), |failure| failure.reason().to_owned());
	let payload = serde_json::json!({
		"errorClass": error_class,
		"accountFingerprint": account_fingerprint,
		"email": email,
		"reason": reason,
	});

	if let Err(record_error) = recorder.record(event_type, &payload.to_string()) {
		tracing::warn!(
			?record_error,
			event_type,
			error_class,
			"Failed to record Codex account failure event."
		);
	}
}

pub(super) fn start_or_resume_thread_session(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
) -> Result<ThreadSessionResponse> {
	if let Some(resume_thread_id) = request.resume_thread_id.as_deref() {
		return resume_existing_thread_session(client, recorder, request, resume_thread_id);
	}

	start_fresh_thread_session(client, recorder, request)
}

pub(super) fn record_thread_session_start(
	state_store: &StateStore,
	request: &AppServerRunRequest<'_>,
	recorder: &mut RunRecorder<'_>,
	thread_id: &str,
	effective_thread_config: &EffectiveThreadConfig,
) -> Result<()> {
	state_store.update_run_thread(&request.run_id, thread_id)?;
	recorder.set_thread_id(thread_id)?;
	recorder.set_effective_runtime(effective_thread_config)?;

	validate_effective_thread_config(&request.cwd, effective_thread_config)?;

	recorder.mark_activity()
}

pub(super) fn build_thread_start_request(
	request: &AppServerRunRequest<'_>,
) -> Result<ThreadStartRequest> {
	let dynamic_tools = request
		.dynamic_tool_handler
		.map(validated_dynamic_tool_specs)
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

pub(super) fn build_thread_resume_request(
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

pub(super) fn login_account_params(account: &CodexAccountLogin) -> LoginAccountParams {
	LoginAccountParams::ChatgptAuthTokens {
		access_token: account.access_token().to_owned(),
		chatgpt_account_id: account.account_id().to_owned(),
		chatgpt_plan_type: account.plan_type().map(str::to_owned),
	}
}

pub(super) fn validate_effective_thread_config(
	cwd: &str,
	runtime: &EffectiveThreadConfig,
) -> Result<()> {
	if runtime.cwd != cwd {
		eyre::bail!(
			"app_server_protocol_failure: effective cwd `{}` did not match requested worktree `{cwd}`.",
			runtime.cwd
		);
	}
	if runtime.approval_policy != "never" {
		eyre::bail!(
			"app_server_protocol_failure: effective approval policy `{}` is interactive; Decodex requires `never`.",
			runtime.approval_policy
		);
	}
	if runtime.sandbox_mode == "readOnly" {
		eyre::bail!(
			"app_server_protocol_failure: effective sandbox mode `readOnly` does not allow Decodex execution."
		);
	}

	Ok(())
}

pub(super) fn validate_initialize_codex_home(
	expected: &ResolvedAppServerCodexHomeEnv,
	response: &InitializeResponse,
) -> Result<()> {
	let expected_home = normalized_home_path(expected.codex_home());
	let resolved_home = normalized_home_path(Path::new(&response.codex_home));

	if resolved_home != expected_home {
		tracing::warn!(
			expected_codex_home = %expected.codex_home().display(),
			resolved_codex_home = %response.codex_home,
			"Codex app-server resolved an unexpected Codex home."
		);

		return Err(Report::new(AppServerHomePreflightFailure::initialize_mismatch(
			response.codex_home.clone(),
			expected.codex_home().display().to_string(),
		)));
	}

	Ok(())
}

pub(super) fn thread_resume_error_allows_fallback(error: &Report) -> bool {
	let message = error.to_string().to_lowercase();

	thread_missing_error_message_allows_discard(&message)
}

pub(super) fn thread_missing_error_message_allows_discard(message: &str) -> bool {
	message.contains("no rollout found for thread id") || message.contains("thread not found")
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
		Err(error) if thread_resume_error_allows_fallback(&error) => {
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

fn record_codex_account_login(
	recorder: &mut RunRecorder<'_>,
	summary: &CodexAccountActivitySummary,
) -> Result<()> {
	recorder.record(
		"account/login/start",
		&serde_json::json!({
			"type": "chatgptAuthTokens",
			"accountFingerprint": summary.account_fingerprint.as_str(),
			"planType": summary.plan_type.as_deref(),
			"status": summary.status.as_str(),
			"refreshStatus": summary.refresh_status.as_str(),
			"primaryRemainingPercent": summary.primary_remaining_percent,
			"secondaryRemainingPercent": summary.secondary_remaining_percent,
			"rateLimitReachedType": summary.rate_limit_reached_type.as_deref(),
		})
		.to_string(),
	)
}

fn normalized_home_path(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
