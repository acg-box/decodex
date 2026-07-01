#[allow(clippy::wildcard_imports)] use super::*;
use super::{
	checks::{preflight_error_timed_out, record_app_server_preflight_report},
	report::check_name_for_method,
};

pub(in crate::agent::app_server) fn plugin_list_params_for_preflight(
	cwd: &str,
) -> PluginListParams {
	PluginListParams {
		cwds: Some(vec![cwd.to_owned()]),
		marketplace_kinds: Some(vec![PREFLIGHT_PLUGIN_MARKETPLACE_KIND.to_owned()]),
	}
}

pub(super) fn preflight_method_failure<T>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request_timeout: Duration,
	attempt_count: u32,
	error: Report,
) -> crate::prelude::Result<T> {
	let error_message = error.to_string();
	let timed_out = preflight_error_timed_out(&error);
	let retry_count = attempt_count.saturating_sub(1);
	let mut failed_report = report.clone();
	let mut details = BTreeMap::new();

	details.insert(String::from("method"), method.to_owned());
	details.insert(String::from("error"), error_message.clone());
	details.insert(String::from("attempt_count"), attempt_count.to_string());

	if retry_count > 0 {
		details.insert(String::from("retry_count"), retry_count.to_string());
	}
	if timed_out {
		details.insert(String::from("failure_reason"), String::from("timeout"));
		details.insert(String::from("timeout_seconds"), request_timeout.as_secs().to_string());
	}

	failed_report.push_blocked(
		check_name_for_method(method),
		if timed_out {
			format!("`{method}` timed out before thread/start after {attempt_count} attempts.")
		} else {
			format!("`{method}` failed before thread/start.")
		},
		details,
	);

	record_app_server_preflight_report(recorder, &failed_report)?;

	let failure = if timed_out {
		AppServerCapabilityPreflightFailure::method_timed_out(method, error_message, failed_report)
	} else {
		AppServerCapabilityPreflightFailure::method_failed(method, error_message, failed_report)
	};

	Err(Report::new(failure))
}

pub(in crate::agent::app_server) fn preflight_request<T, F>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request: F,
) -> crate::prelude::Result<T>
where
	F: FnOnce() -> crate::prelude::Result<T>,
{
	match request() {
		Ok(response) => Ok(response),
		Err(error) => preflight_method_failure(recorder, report, method, REQUEST_TIMEOUT, 1, error),
	}
}

pub(in crate::agent::app_server) fn preflight_request_with_timeout_retry<T, F>(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
	method: &'static str,
	request_timeout: Duration,
	max_attempts: u32,
	mut request: F,
) -> crate::prelude::Result<T>
where
	F: FnMut() -> crate::prelude::Result<T>,
{
	let max_attempts = max_attempts.max(1);
	let mut attempt_count = 1;

	loop {
		match request() {
			Ok(response) => return Ok(response),
			Err(error) if preflight_error_timed_out(&error) && attempt_count < max_attempts => {
				tracing::warn!(
					method,
					attempt = attempt_count,
					max_attempts,
					"Retrying app-server preflight method after timeout."
				);

				attempt_count += 1;
			},
			Err(error) => {
				return preflight_method_failure(
					recorder,
					report,
					method,
					request_timeout,
					attempt_count,
					error,
				);
			},
		}
	}
}

pub(super) fn list_all_models_for_preflight(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
) -> crate::prelude::Result<Vec<ModelSummary>> {
	let mut cursor = None;
	let mut models = Vec::new();

	loop {
		let response: ModelListResponse =
			preflight_request(recorder, report, "model/list", || {
				client.list_models(&ModelListParams {
					cursor: cursor.clone(),
					include_hidden: Some(true),
					limit: Some(PREFLIGHT_MODEL_PAGE_LIMIT),
				})
			})?;

		models.extend(response.data);

		let Some(next_cursor) = response.next_cursor else {
			return Ok(models);
		};

		cursor = Some(next_cursor);
	}
}

pub(super) fn list_all_mcp_servers_for_preflight(
	client: &mut AppServerClient,
) -> crate::prelude::Result<Vec<McpServerStatusSummary>> {
	let mut cursor = None;
	let mut servers = Vec::new();

	loop {
		let response: ListMcpServerStatusResponse = client.list_mcp_server_status(
			&ListMcpServerStatusParams {
				cursor: cursor.clone(),
				detail: Some(PREFLIGHT_MCP_DETAIL.to_owned()),
				limit: Some(PREFLIGHT_MCP_PAGE_LIMIT),
			},
			MCP_PREFLIGHT_REQUEST_TIMEOUT,
		)?;

		servers.extend(response.data);

		let Some(next_cursor) = response.next_cursor else {
			return Ok(servers);
		};

		cursor = Some(next_cursor);
	}
}
