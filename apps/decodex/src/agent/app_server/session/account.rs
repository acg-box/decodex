use color_eyre::Report;

use crate::{
	agent::{
		app_server::{
			protocol::{AppServerClient, LoginAccountParams, LoginAccountResponse},
			runtime_types::{
				AppServerRunRequest, RequestDispatchContext, RequestWaitPhase, RunRecorder,
			},
			server_requests, transport,
		},
		codex_accounts::{CodexAccountAuthFailure, CodexAccountLogin},
	},
	prelude::Result,
	state::CodexAccountActivitySummary,
};

pub(in crate::agent::app_server) fn login_codex_account_for_run(
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

pub(in crate::agent::app_server) fn record_codex_account_failure(
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

pub(in crate::agent::app_server::session) fn login_account_params(
	account: &CodexAccountLogin,
) -> LoginAccountParams {
	LoginAccountParams::ChatgptAuthTokens {
		access_token: account.access_token().to_owned(),
		chatgpt_account_id: account.account_id().to_owned(),
		chatgpt_plan_type: account.plan_type().map(str::to_owned),
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
