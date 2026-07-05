use crate::orchestrator::tests::operator::status::http::{
	self, ProjectRegistration, StateStore, Value, fs, orchestrator,
};

#[test]
fn operator_lane_inspect_api_returns_lane_identity() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree should exist");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("pub-101-attempt-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.update_run_thread("pub-101-attempt-1", "thread-1").expect("thread should record");
	state_store.update_run_turn("pub-101-attempt-1", "turn-1").expect("turn should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "pub-101-attempt-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let response = String::from_utf8(orchestrator::build_operator_lane_inspect_http_response(
		&state_store,
		format!(
			"GET {}?projectId=pubfi&issue=PUB-101&runId=pub-101-attempt-1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
			orchestrator::OPERATOR_LANE_INSPECT_ENDPOINT_PATH
		)
		.as_bytes(),
	))
	.expect("lane inspect response should be utf-8");
	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.expect("lane inspect response should include body");
	let data: Value = serde_json::from_str(body).expect("lane inspect response should be json");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(data["projectId"], "pubfi");
	assert_eq!(data["issue"], "PUB-101");
	assert_eq!(data["matchedRunCount"], 1);
	assert_eq!(data["runs"][0]["runId"], "pub-101-attempt-1");
	assert_eq!(data["runs"][0]["attemptStatus"], "running");
	assert_eq!(data["runs"][0]["runLease"], true);
	assert_eq!(data["runs"][0]["ownershipState"], "leased_run");
	assert_eq!(data["runs"][0]["livenessState"], "unknown");
	assert_eq!(data["runs"][0]["policyState"], "allowed");
	assert_eq!(data["runs"][0]["terminalizationState"], "none");
	assert_eq!(data["runs"][0]["laneControlNextAction"], "continue_owned_attempt");
	assert_eq!(data["runs"][0]["threadId"], "thread-1");
	assert_eq!(data["runs"][0]["turnId"], "turn-1");
	assert_eq!(data["runs"][0]["softInterruptAvailable"], false);
	assert_eq!(data["runs"][0]["hardInterruptRequiresForce"], true);
}
