use crate::orchestrator::tests::operator::status::http::{
	OperatorControlRequests, Value, orchestrator,
};

#[test]
fn operator_state_endpoint_queues_linear_scan_request() {
	let control_requests = OperatorControlRequests::default();
	let body = br#"{"projectId":"pubfi"}"#;
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LINEAR_SCAN_ENDPOINT_PATH,
		body.len(),
		String::from_utf8_lossy(body)
	);
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response_with_controls(
			request.as_bytes(),
			&control_requests,
		)
		.expect("linear scan response should build"),
	)
	.expect("linear scan response should be utf-8");
	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.expect("linear scan response should include body");
	let data: Value = serde_json::from_str(body).expect("linear scan response should be json");

	assert!(response.starts_with("HTTP/1.1 202 Accepted\r\n"));
	assert_eq!(data["status"], "queued");
	assert_eq!(data["scope"], "pubfi");
	assert_eq!(
		control_requests.drain_linear_scan_requests().expect("linear scan requests should drain"),
		vec![orchestrator::OperatorLinearScanRequest { project_id: Some(String::from("pubfi")) }]
	);
}
