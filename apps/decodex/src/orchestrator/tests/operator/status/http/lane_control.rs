use crate::orchestrator::tests::operator::status::http::{
	self, Arc, Command, DashboardEventHub, Mutex, OperatorControlRequests, ProjectRegistration,
	PublishedOperatorSnapshot, RUN_CONTROL_CHANNEL_DIR, RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
	Read as _, RunLeaseMissingControlFixture, ServiceConfig, Shutdown, StateStore, TcpListener,
	TcpStream, Value, Write, fs, orchestrator, state, thread,
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

#[test]
fn operator_lane_inspect_projects_terminal_ledger_for_unowned_stale_run() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("Done", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree should exist");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("pub-101-attempt-1", &issue.id, 1, "running")
		.expect("stale running attempt should record");
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
	assert_eq!(data["runs"][0]["status"], "cleanup_complete");
	assert_eq!(data["runs"][0]["attemptStatus"], "cleanup_complete");
	assert_eq!(data["runs"][0]["phase"], "completed");
	assert_eq!(data["runs"][0]["currentOperation"], "ledger_outcome");
	assert_eq!(data["runs"][0]["runLease"], false);
	assert_eq!(data["runs"][0]["livenessState"], "not_running");
	assert_eq!(data["runs"][0]["ownershipState"], "closed");
}

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

#[test]
fn operator_lane_inspect_api_filters_by_run_id() {
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
			"GET {}?projectId=pubfi&issue=PUB-101&runId=pub-101-attempt-2 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
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

	assert!(response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
	assert!(
		data["error"]
			.as_str()
			.unwrap_or_default()
			.contains("No local lane matched issue `PUB-101` and run `pub-101-attempt-2`")
	);
}

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

#[test]
fn operator_lane_interrupt_api_force_reports_hard_fallback_after_pending_soft_interrupt() {
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
	let body =
		br#"{"projectId":"pubfi","issue":"PUB-101","runId":"pub-101-attempt-1","force":true}"#;
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		body.len(),
		String::from_utf8_lossy(body)
	);

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

	let channel_path =
		worktree_path.join(RUN_CONTROL_CHANNEL_DIR).join("pub-101-attempt-1.channel");

	fs::create_dir_all(channel_path.parent().expect("channel path should have parent"))
		.expect("run-control channel dir should exist");
	fs::write(&channel_path, "ready\n").expect("control channel should write");

	state_store
		.publish_run_control_channel_for_active_attempt(
			"pub-101-attempt-1",
			1,
			&channel_path,
			RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		)
		.expect("control channel should publish");

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

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(data["classification"], "hard_interrupt_fallback");
	assert_eq!(data["softInterrupt"]["status"], "pending");
	assert_eq!(data["hardInterrupt"]["status"], "unavailable");
	assert!(
		data["nextAction"].as_str().unwrap_or_default().contains("Hard fallback was unavailable")
	);
}

#[test]
fn operator_lane_interrupt_api_force_does_not_hard_fallback_after_control_rejection() {
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
	let body =
		br#"{"projectId":"pubfi","issue":"PUB-101","runId":"pub-101-attempt-1","force":true}"#;
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		body.len(),
		String::from_utf8_lossy(body)
	);

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
	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, "pub-101-attempt-1", 1)
		.expect("private control audit should read");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(data["force"], true);
	assert_eq!(data["classification"], "control_request_rejected");
	assert_eq!(data["softInterrupt"]["status"], "rejected");
	assert_eq!(data["softInterrupt"]["errorClass"], "control_channel_missing");
	assert_eq!(data["hardInterrupt"], Value::Null);
	assert!(events.iter().any(|event| {
		event.event_type() == "control_action"
			&& event.payload()["reason"] == "control_channel_missing"
	}));
}

#[cfg(unix)]
fn run_lease_missing_control_fixture(
	config: &ServiceConfig,
	state_store: &StateStore,
) -> RunLeaseMissingControlFixture {
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let channel_path =
		worktree_path.join(RUN_CONTROL_CHANNEL_DIR).join("pub-101-attempt-1.channel");
	let child = Command::new("/bin/sh")
		.args(["-c", "exec sleep 60"])
		.spawn()
		.expect("lane child process should start");
	let child_process_id = child.id();

	assert!(
		orchestrator::process_is_alive(child_process_id),
		"lane child process should be live before control request"
	);

	fs::create_dir_all(channel_path.parent().expect("channel path should have parent"))
		.expect("run-control channel dir should exist");
	fs::write(&channel_path, "ready\n").expect("control channel should write");
	state::write_run_activity_marker_for_process(
		&worktree_path,
		"pub-101-attempt-1",
		1,
		child_process_id,
	)
	.expect("activity marker should write");

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
	state_store
		.publish_run_control_channel_for_active_attempt(
			"pub-101-attempt-1",
			1,
			&channel_path,
			RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		)
		.expect("control channel should publish");
	state_store.clear_lease(&issue.id).expect("lease should clear");

	RunLeaseMissingControlFixture { issue, channel_path, child, child_process_id }
}

#[cfg(unix)]
fn operator_json_response_body(response: &str, context: &str) -> Value {
	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.unwrap_or_else(|| http::panic!("{context} response should include body"));

	serde_json::from_str(body).unwrap_or_else(|_| http::panic!("{context} response should be json"))
}

#[cfg(unix)]
fn reap_run_lease_missing_child(fixture: &mut RunLeaseMissingControlFixture) {
	if orchestrator::process_is_alive(fixture.child_process_id) {
		fixture.child.kill().expect("lane child process should be killable after failed fallback");
	}

	fixture.child.wait().expect("lane child process should reap");
}

#[cfg(unix)]
fn assert_run_lease_missing_control_audit(
	config: &ServiceConfig,
	state_store: &StateStore,
	fixture: &RunLeaseMissingControlFixture,
	run_id: &str,
) {
	let events = state_store
		.list_private_execution_events(config.service_id(), &fixture.issue.id, run_id, 1)
		.expect("private control audit should read");
	let missing_lease_steer_event = events
		.iter()
		.find(|event| {
			event.event_type() == "control_action"
				&& event.payload()["action"] == "steer"
				&& event.payload()["reason"] == "run_lease_missing"
		})
		.expect("run lease steer rejection should be audited");
	let missing_lease_interrupt_event = events
		.iter()
		.find(|event| {
			event.event_type() == "control_action"
				&& event.payload()["action"] == "interrupt"
				&& event.payload()["reason"] == "run_lease_missing"
		})
		.expect("run lease interrupt rejection should be audited");
	let expected_channel_path = fixture.channel_path.display().to_string();

	assert_eq!(
		missing_lease_steer_event.payload()["context"]["process_alive"].as_bool(),
		Some(true)
	);
	assert_eq!(
		missing_lease_steer_event.payload()["channel"]["channel_path"].as_str(),
		Some(expected_channel_path.as_str())
	);
	assert_eq!(missing_lease_interrupt_event.payload()["lane"]["run_lease"].as_bool(), Some(false));
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["process_alive"].as_bool(),
		Some(true)
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["execution_liveness"].as_str(),
		Some("process_alive")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["ownership_state"].as_str(),
		Some("orphaned_live_thread")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["liveness_state"].as_str(),
		Some("process_alive")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["lane_control_next_action"].as_str(),
		Some("inspect_or_interrupt_orphaned_live_thread")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["lane_control_conditions"][0].as_str(),
		Some("run_lease_missing")
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["context"]["control_capability"]["channel_path"]
			.as_str(),
		Some(expected_channel_path.as_str())
	);
	assert_eq!(
		missing_lease_interrupt_event.payload()["channel"]["channel_path"].as_str(),
		Some(expected_channel_path.as_str())
	);
	assert!(events.iter().any(|event| {
		event.event_type() == "control_action"
			&& event.payload()["outcome"] == "fallback"
			&& event.payload()["reason"] == "hard_interrupt_fallback"
	}));
}

#[cfg(unix)]
#[test]
fn operator_lane_interrupt_api_force_hard_fallbacks_after_run_lease_missing() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut fixture = run_lease_missing_control_fixture(&config, &state_store);
	let project_id = config.service_id().to_owned();
	let issue_identifier = fixture.issue.identifier.clone();
	let run_id = "pub-101-attempt-1";
	let body = format!(
		r#"{{"projectId":"{project_id}","issue":"{issue_identifier}","runId":"{run_id}","force":true}}"#
	);
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		body.len(),
		body
	);
	let steer_body = format!(
		r#"{{"projectId":"{project_id}","issue":"{issue_identifier}","runId":"{run_id}","expectedTurnId":"turn-1","message":"preserve partial work and report current state"}}"#
	);
	let steer_request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_STEER_ENDPOINT_PATH,
		steer_body.len(),
		steer_body
	);
	let steer_response = String::from_utf8(orchestrator::build_operator_lane_steer_http_response(
		&state_store,
		steer_request.as_bytes(),
	))
	.expect("lane steer response should be utf-8");
	let steer_data = operator_json_response_body(&steer_response, "lane steer");
	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		&state_store,
		request.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let data = operator_json_response_body(&response, "lane interrupt");

	reap_run_lease_missing_child(&mut fixture);

	assert!(steer_response.starts_with("HTTP/1.1 409 Conflict\r\n"), "{steer_response}");
	assert_eq!(steer_data["outcome"], "rejected");
	assert_eq!(steer_data["reason"], "run_lease_missing");
	assert_eq!(steer_data["failureClass"], "run_control_action_failed");
	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(data["classification"], "hard_interrupt_fallback");
	assert_eq!(data["softInterrupt"]["status"], "rejected");
	assert_eq!(data["softInterrupt"]["errorClass"], "run_lease_missing");
	assert_eq!(data["hardInterrupt"]["classification"], "hard_interrupt_fallback");
	assert_eq!(data["hardInterrupt"]["status"], "sent");
	assert_eq!(
		data["hardInterrupt"]["processId"].as_u64(),
		Some(u64::from(fixture.child_process_id))
	);
	assert_eq!(data["hardInterrupt"]["processAliveAfter"], false);

	assert_run_lease_missing_control_audit(&config, &state_store, &fixture, run_id);
}

#[cfg(unix)]
#[test]
fn operator_lane_interrupt_api_force_hard_fallbacks_terminal_live_process_without_soft_owner() {
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
	let project_id = config.service_id().to_owned();
	let issue_identifier = issue.identifier.clone();
	let run_id = "pub-101-attempt-1";
	let body = format!(
		r#"{{"projectId":"{project_id}","issue":"{issue_identifier}","runId":"{run_id}","force":true}}"#
	);
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		body.len(),
		body
	);
	let child = Command::new("/bin/sh")
		.args(["-c", "exec sleep 60"])
		.spawn()
		.expect("lane child process should start");
	let child_process_id = child.id();
	let mut child = child;

	fs::create_dir_all(&worktree_path).expect("worktree should exist");
	state::write_run_activity_marker_for_process(&worktree_path, run_id, 1, child_process_id)
		.expect("activity marker should write");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt(run_id, &issue.id, 1, "succeeded")
		.expect("run attempt should record");
	state_store.update_run_thread(run_id, "thread-1").expect("thread should record");
	state_store.update_run_turn(run_id, "turn-1").expect("turn should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		&state_store,
		request.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let data = operator_json_response_body(&response, "lane interrupt");

	if orchestrator::process_is_alive(child_process_id) {
		child.kill().expect("lane child process should be killable after failed fallback");
	}

	child.wait().expect("lane child process should reap");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
	assert_eq!(data["classification"], "hard_interrupt_fallback");
	assert_eq!(data["softInterrupt"]["status"], "unavailable");
	assert_eq!(data["softInterrupt"]["errorClass"], "lane_not_active");
	assert_eq!(data["hardInterrupt"]["classification"], "hard_interrupt_fallback");
	assert_eq!(data["hardInterrupt"]["status"], "sent");
	assert_eq!(data["hardInterrupt"]["processId"].as_u64(), Some(u64::from(child_process_id)));
	assert_eq!(data["hardInterrupt"]["processAliveAfter"], false);
}

#[test]
fn operator_lane_steer_api_rejects_stale_expected_turn_id() {
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
	let body = br#"{"projectId":"pubfi","issue":"PUB-101","runId":"pub-101-attempt-1","expectedTurnId":"turn-old","message":"please adjust priority"}"#;
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_STEER_ENDPOINT_PATH,
		body.len(),
		String::from_utf8_lossy(body)
	);

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

	let channel_path =
		worktree_path.join(RUN_CONTROL_CHANNEL_DIR).join("pub-101-attempt-1.channel");

	fs::create_dir_all(channel_path.parent().expect("channel path should have parent"))
		.expect("run-control channel dir should exist");
	fs::write(&channel_path, "ready\n").expect("control channel should write");

	state_store
		.publish_run_control_channel_for_active_attempt(
			"pub-101-attempt-1",
			1,
			&channel_path,
			RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		)
		.expect("control channel should publish");

	let response = String::from_utf8(orchestrator::build_operator_lane_steer_http_response(
		&state_store,
		request.as_bytes(),
	))
	.expect("lane steer response should be utf-8");
	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.expect("lane steer response should include body");
	let data: Value = serde_json::from_str(body).expect("lane steer response should be json");
	let events = state_store
		.list_private_execution_events(config.service_id(), &issue.id, "pub-101-attempt-1", 1)
		.expect("private control audit should read");

	assert!(response.starts_with("HTTP/1.1 409 Conflict\r\n"));
	assert_eq!(data["outcome"], "rejected");
	assert_eq!(data["reason"], "turn_mismatch");
	assert_eq!(data["failureClass"], "stale_expected_turn_id");
	assert_eq!(data["expectedTurnId"], "turn-old");
	assert_eq!(data["currentTurnId"], "turn-1");
	assert!(events.iter().any(|event| {
		event.event_type() == "control_action"
			&& event.payload()["action"] == "steer"
			&& event.payload()["failure_class"] == "stale_expected_turn_id"
	}));
}

#[test]
fn operator_lane_steer_endpoint_accepts_large_operator_message_body() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let channel_path =
		worktree_path.join(RUN_CONTROL_CHANNEL_DIR).join("pub-101-attempt-1.channel");

	fs::create_dir_all(channel_path.parent().expect("channel path should have parent"))
		.expect("run-control channel dir should exist");
	fs::write(&channel_path, "ready\n").expect("control channel should write");

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
	state_store
		.publish_run_control_channel_for_active_attempt(
			"pub-101-attempt-1",
			1,
			&channel_path,
			RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
		)
		.expect("control channel should publish");

	let body = serde_json::json!({
		"projectId": "pubfi",
		"issue": "PUB-101",
		"runId": "pub-101-attempt-1",
		"expectedTurnId": "turn-old",
		"message": "x".repeat(12 * 1_024),
	})
	.to_string();
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server = thread::spawn(move || {
		let (stream, _) = listener.accept().expect("listener should accept a connection");
		let dashboard_events = DashboardEventHub::default();

		orchestrator::handle_operator_state_endpoint_connection(
			stream,
			&server_snapshot,
			&dashboard_events,
			&OperatorControlRequests::default(),
			&server_state_store,
		)
		.expect("handler should accept broad steer request body");
	});
	let request = format!(
		"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		orchestrator::OPERATOR_LANE_STEER_ENDPOINT_PATH,
		body.len(),
		body
	);
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = String::new();

	client.write_all(request.as_bytes()).expect("client should write request");
	client.shutdown(Shutdown::Write).expect("client should close request body");
	client.read_to_string(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.expect("lane steer response should include body");
	let data: Value = serde_json::from_str(body).expect("lane steer response should be json");

	assert!(response.starts_with("HTTP/1.1 409 Conflict\r\n"));
	assert_eq!(data["failureClass"], "stale_expected_turn_id");
	assert_eq!(data["messageByteCount"], 12 * 1_024);
}
