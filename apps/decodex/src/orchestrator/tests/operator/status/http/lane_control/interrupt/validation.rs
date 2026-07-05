use crate::orchestrator::tests::operator::status::http::{
	self, ProjectRegistration, StateStore, Value, orchestrator,
};

#[test]
fn operator_lane_interrupt_api_rejects_blank_run_id() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let body = br#"{"projectId":"pubfi","issue":"PUB-101","runId":""}"#;
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		body.len(),
		String::from_utf8_lossy(body)
	);

	state_store.upsert_project(&registration).expect("project should register");

	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		&state_store,
		request.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.expect("lane interrupt response should include body");
	let data: Value = serde_json::from_str(body).expect("lane interrupt response should be json");

	assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
	assert!(data["error"].as_str().unwrap_or_default().contains("runId"));
}
