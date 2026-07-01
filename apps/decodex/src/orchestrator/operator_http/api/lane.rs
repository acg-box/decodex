use std::time::Duration;

use super::super::{
	DEFAULT_STEER_RESULT_WAIT_TIMEOUT, LaneSteerReport, LaneSteerRequest,
	OperatorLaneInterruptHttpRequest, OperatorLaneSteerHttpRequest, Report, Result, ServiceConfig,
	StateStore, eyre, http_response_bytes, json, lane_control, operator_http_query_value,
	operator_http_query_value_alias, operator_http_request_body,
};

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
