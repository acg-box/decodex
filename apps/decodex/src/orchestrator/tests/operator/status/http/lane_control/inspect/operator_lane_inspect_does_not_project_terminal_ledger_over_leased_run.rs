use crate::orchestrator::tests::operator::status::http::{
	self, ProjectRegistration, StateStore, Value, fs, orchestrator,
};

#[test]
fn operator_lane_inspect_does_not_project_terminal_ledger_over_leased_run() {
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
		.expect("running attempt should record");
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

	http::seed_local_linear_execution_events(
		&state_store,
		&http::successful_linear_execution_history_comments_with_cleanup(&issue),
	);

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
	assert_eq!(data["matchedRunCount"], 1);
	assert_eq!(data["runs"][0]["runId"], "pub-101-attempt-1");
	assert_eq!(data["runs"][0]["status"], "running");
	assert_eq!(data["runs"][0]["attemptStatus"], "running");
	assert_eq!(data["runs"][0]["runLease"], true);
	assert_eq!(data["runs"][0]["ownershipState"], "leased_run");
	assert_eq!(data["runs"][0]["currentOperation"], "agent_run");
}
