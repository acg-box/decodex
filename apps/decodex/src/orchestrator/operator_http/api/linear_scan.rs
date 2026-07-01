use super::super::{
	OperatorControlRequests, OperatorLinearScanHttpRequest, Result, eyre, http_response_bytes,
	json, operator_http_request_body,
};

pub(in crate::orchestrator::operator_http) fn build_operator_linear_scan_http_response(
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
