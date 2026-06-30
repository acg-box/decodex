use std::time::Duration;

use super::{
	AccountUseRequest, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, LaneSteerReport, LaneSteerRequest,
	OperatorAccountRequest, OperatorControlRequests, OperatorLaneInterruptHttpRequest,
	OperatorLaneSteerHttpRequest, OperatorLinearScanHttpRequest, OperatorRequestRoute, Path,
	PathBuf, Report, Result, ServiceConfig, StateStore, accounts, eyre, http_response_bytes, json,
	lane_control, operator_http_query_value, operator_http_query_value_alias,
	operator_http_request_body,
};
#[cfg(test)]
use super::{build_operator_state_http_response_for_route, parse_operator_state_request_route};

#[cfg(test)]
pub(crate) fn build_operator_state_http_response(request: &[u8]) -> Result<Vec<u8>> {
	let control_requests = OperatorControlRequests::default();

	build_operator_state_http_response_with_control_requests(request, &control_requests)
}

#[cfg(test)]
pub(crate) fn build_operator_state_http_response_with_control_requests(
	request: &[u8],
	control_requests: &OperatorControlRequests,
) -> Result<Vec<u8>> {
	let route = match parse_operator_state_request_route(request) {
		Ok(route) => route,
		Err(response) => return Ok(response),
	};

	if operator_request_route_is_account_api(&route) {
		return Ok(build_operator_account_http_response(route, request));
	}
	if route == OperatorRequestRoute::LinearScan {
		return Ok(build_operator_linear_scan_http_response(control_requests, request));
	}

	Ok(build_operator_state_http_response_for_route(route))
}

pub(super) fn operator_request_route_is_account_api(route: &OperatorRequestRoute) -> bool {
	matches!(
		route,
		OperatorRequestRoute::AccountList { .. }
			| OperatorRequestRoute::AccountSelect
			| OperatorRequestRoute::AccountClear
			| OperatorRequestRoute::AccountLogout
			| OperatorRequestRoute::AccountImport
			| OperatorRequestRoute::AccountUse
			| OperatorRequestRoute::AccountRerollName
	)
}

pub(super) fn build_operator_account_http_response(
	route: OperatorRequestRoute,
	request: &[u8],
) -> Vec<u8> {
	match operator_account_http_response_body(route, request) {
		Ok(body) => http_response_bytes("200 OK", "application/json", &body),
		Err(error) => {
			let body = serde_json::to_vec(&json!({ "error": error.to_string() }))
				.unwrap_or_else(|_| br#"{"error":"account request failed"}"#.to_vec());

			http_response_bytes("400 Bad Request", "application/json", &body)
		},
	}
}

pub(super) fn operator_account_http_response_body(
	route: OperatorRequestRoute,
	request: &[u8],
) -> Result<Vec<u8>> {
	match route {
		OperatorRequestRoute::AccountList { force_refresh } =>
			serde_json::to_vec(&accounts::account_list_with_cached_usage(force_refresh)?)
				.map_err(Into::into),
		OperatorRequestRoute::AccountSelect => {
			let selector = operator_account_request_selector(request)?;
			let response =
				accounts::hydrate_account_list_usage(accounts::account_select(&selector)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountClear => {
			let response = accounts::hydrate_account_list_usage(accounts::account_clear()?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountLogout => {
			let selector = operator_account_request_selector(request)?;
			let response =
				accounts::hydrate_account_list_usage(accounts::account_logout(&selector)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountImport => {
			let body = operator_account_request_body(request)?;
			let auth_json_path = body
				.auth_json_path
				.as_deref()
				.filter(|path| !path.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account import requires auth_json_path."))?;
			let response = accounts::hydrate_account_list_usage(accounts::account_import(
				Path::new(auth_json_path),
			)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountUse => {
			let body = operator_account_request_body(request)?;
			let selector = body
				.selector
				.as_deref()
				.filter(|selector| !selector.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account use requires selector."))?;
			let auth_json_path = body.auth_json_path.as_deref().map(PathBuf::from);
			let response = accounts::account_use(&AccountUseRequest {
				selector: selector.to_owned(),
				auth_json_path,
				json: true,
			})?;

			serde_json::to_vec(&response).map_err(Into::into)
		},
		OperatorRequestRoute::AccountRerollName => {
			let body = operator_account_request_body(request)?;
			let selector = body
				.selector
				.as_deref()
				.filter(|selector| !selector.trim().is_empty())
				.ok_or_else(|| eyre::eyre!("Account name reroll requires selector."))?;
			let response = accounts::hydrate_account_list_usage(accounts::account_reroll_name(
				selector,
				body.random_name_offset,
			)?);

			serde_json::to_vec(&response).map_err(Into::into)
		},
		_ => eyre::bail!("Unsupported account API route."),
	}
}

pub(super) fn operator_account_request_selector(request: &[u8]) -> Result<String> {
	let body = operator_account_request_body(request)?;

	body.selector
		.filter(|selector| !selector.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("Account request requires selector."))
}

pub(super) fn operator_account_request_body(request: &[u8]) -> Result<OperatorAccountRequest> {
	let body = operator_http_request_body(request)?;

	if body.is_empty() {
		return Ok(OperatorAccountRequest {
			selector: None,
			auth_json_path: None,
			random_name_offset: None,
		});
	}

	serde_json::from_slice(body)
		.map_err(|error| eyre::eyre!("Account request body was not valid JSON: {error}"))
}

pub(super) fn build_operator_linear_scan_http_response(
	control_requests: &OperatorControlRequests,
	request: &[u8],
) -> Vec<u8> {
	match operator_linear_scan_http_response_body(control_requests, request) {
		Ok(body) => http_response_bytes("202 Accepted", "application/json", &body),
		Err(error) => {
			let body = serde_json::to_vec(&json!({ "error": error.to_string() }))
				.unwrap_or_else(|_| br#"{"error":"linear scan request failed"}"#.to_vec());

			http_response_bytes("400 Bad Request", "application/json", &body)
		},
	}
}

pub(super) fn operator_linear_scan_http_response_body(
	control_requests: &OperatorControlRequests,
	request: &[u8],
) -> Result<Vec<u8>> {
	let project_id = operator_linear_scan_request_project_id(request)?;
	let scope = project_id.as_deref().unwrap_or("all");

	control_requests.request_linear_scan(project_id.clone())?;

	serde_json::to_vec(&json!({
		"status": "queued",
		"scope": scope,
		"project_id": project_id,
		"next_action": "Decodex will run the requested Linear scan on the next control-plane tick unless the tracker connector is rate-limited.",
	}))
	.map_err(Into::into)
}

pub(super) fn operator_linear_scan_request_project_id(request: &[u8]) -> Result<Option<String>> {
	let body = operator_http_request_body(request)?;

	if body.is_empty() {
		return Ok(None);
	}

	let request: OperatorLinearScanHttpRequest = serde_json::from_slice(body)
		.map_err(|error| eyre::eyre!("Linear scan request body was not valid JSON: {error}"))?;

	match request.project_id {
		Some(project_id) if project_id.trim().is_empty() => {
			eyre::bail!("Linear scan request project_id must not be blank.")
		},
		Some(project_id) => Ok(Some(project_id.trim().to_owned())),
		None => Ok(None),
	}
}

pub(crate) fn build_operator_lane_inspect_http_response(
	state_store: &StateStore,
	request: &[u8],
) -> Vec<u8> {
	match operator_lane_inspect_http_response_body(state_store, request) {
		Ok(body) => http_response_bytes("200 OK", "application/json", &body),
		Err(error) => operator_lane_error_http_response(error),
	}
}

pub(super) fn operator_lane_inspect_http_response_body(
	state_store: &StateStore,
	request: &[u8],
) -> Result<Vec<u8>> {
	let project_id = operator_http_query_value_alias(request, "projectId", "project_id")?;
	let issue = operator_http_query_value(request, "issue")?
		.filter(|issue| !issue.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("Lane inspect request requires issue query parameter."))?;
	let run_id = operator_http_query_value_alias(request, "runId", "run_id")?;
	let project = operator_lane_http_project(state_store, project_id.as_deref())?;
	let report = lane_control::build_lane_inspect_report(
		state_store,
		&project,
		issue.trim(),
		run_id.as_deref(),
	)?;

	serde_json::to_vec(&report).map_err(Into::into)
}

pub(crate) fn build_operator_lane_interrupt_http_response(
	state_store: &StateStore,
	request: &[u8],
) -> Vec<u8> {
	match operator_lane_interrupt_http_response_body(state_store, request) {
		Ok((status_line, body)) => http_response_bytes(status_line, "application/json", &body),
		Err(error) => operator_lane_error_http_response(error),
	}
}

pub(super) fn operator_lane_interrupt_http_response_body(
	state_store: &StateStore,
	request: &[u8],
) -> Result<(&'static str, Vec<u8>)> {
	let body = operator_http_request_body(request)?;
	let request: OperatorLaneInterruptHttpRequest = serde_json::from_slice(body)
		.map_err(|error| eyre::eyre!("Lane interrupt request body was not valid JSON: {error}"))?;

	if request.issue.trim().is_empty() {
		eyre::bail!("Lane interrupt request issue must not be blank.");
	}
	if request.run_id.trim().is_empty() {
		eyre::bail!("Lane interrupt request runId must not be blank.");
	}

	let project = operator_lane_http_project(state_store, request.project_id.as_deref())?;
	let report = lane_control::interrupt_lane_with_state(
		state_store,
		&project,
		request.issue.trim(),
		request.run_id.trim(),
		request.force.unwrap_or(false),
		request.reason.as_deref(),
		"http",
	)?;
	let status_line = report.http_status_line();

	Ok((status_line, serde_json::to_vec(&report)?))
}

pub(crate) fn build_operator_lane_steer_http_response(
	state_store: &StateStore,
	request: &[u8],
) -> Vec<u8> {
	match operator_lane_steer_http_response_body(state_store, request) {
		Ok((status_line, body)) => http_response_bytes(status_line, "application/json", &body),
		Err(error) => operator_lane_error_http_response(error),
	}
}

pub(super) fn operator_lane_steer_http_response_body(
	state_store: &StateStore,
	request: &[u8],
) -> Result<(&'static str, Vec<u8>)> {
	let body = operator_http_request_body(request)?;
	let request: OperatorLaneSteerHttpRequest = serde_json::from_slice(body)
		.map_err(|error| eyre::eyre!("Lane steer request body was not valid JSON: {error}"))?;
	let issue = request
		.issue
		.as_deref()
		.or(request.issue_id.as_deref())
		.map(str::trim)
		.filter(|issue| !issue.is_empty())
		.ok_or_else(|| eyre::eyre!("Lane steer request requires issue or issueId."))?;

	if request.run_id.trim().is_empty() {
		eyre::bail!("Lane steer request runId must not be blank.");
	}
	if request.expected_turn_id.trim().is_empty() {
		eyre::bail!("Lane steer request expectedTurnId must not be blank.");
	}
	if request.message.trim().is_empty() {
		eyre::bail!("Lane steer request message must not be blank.");
	}

	let project = operator_lane_http_project(state_store, request.project_id.as_deref())?;
	let wait_timeout = request
		.wait_timeout_ms
		.map(Duration::from_millis)
		.unwrap_or(DEFAULT_STEER_RESULT_WAIT_TIMEOUT);
	let steer_request = LaneSteerRequest {
		config_path: None,
		project_id: None,
		issue,
		run_id: request.run_id.trim(),
		expected_turn_id: request.expected_turn_id.trim(),
		message: &request.message,
		source: "http",
		wait_timeout,
	};
	let report = lane_control::steer_lane_with_state(state_store, &project, &steer_request)?;
	let status_line = if lane_steer_report_is_rejected_or_failed(&report) {
		"409 Conflict"
	} else if report.delivery_status == "queued" {
		"202 Accepted"
	} else {
		"200 OK"
	};

	Ok((status_line, serde_json::to_vec(&report)?))
}

pub(super) fn lane_steer_report_is_rejected_or_failed(report: &LaneSteerReport) -> bool {
	matches!(report.outcome.as_str(), "rejected" | "failed" | "timed_out" | "fallback")
}

pub(super) fn operator_lane_http_project(
	state_store: &StateStore,
	project_id: Option<&str>,
) -> Result<ServiceConfig> {
	let registrations = state_store.list_projects()?;
	let registration = match project_id.map(str::trim).filter(|id| !id.is_empty()) {
		Some(project_id) => registrations
			.iter()
			.find(|registration| registration.service_id() == project_id)
			.ok_or_else(|| eyre::eyre!("Decodex project `{project_id}` is not registered."))?,
		None => {
			let enabled = registrations
				.iter()
				.filter(|registration| registration.enabled())
				.collect::<Vec<_>>();

			if enabled.len() == 1 {
				enabled[0]
			} else {
				eyre::bail!(
					"Lane API request requires projectId when zero or multiple projects are enabled."
				);
			}
		},
	};

	ServiceConfig::from_path(registration.config_path())
}

pub(super) fn operator_lane_error_http_response(error: Report) -> Vec<u8> {
	let body = serde_json::to_vec(&json!({ "error": error.to_string() }))
		.unwrap_or_else(|_| br#"{"error":"lane request failed"}"#.to_vec());

	http_response_bytes("400 Bad Request", "application/json", &body)
}
