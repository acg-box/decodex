use crate::orchestrator::tests::operator::status::http::{
	self, Arc, DashboardEventHub, Mutex, OperatorControlRequests, ProjectRegistration,
	PublishedOperatorSnapshot, RUN_CONTROL_CHANNEL_DIR, RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE,
	Read as _, Shutdown, StateStore, TcpListener, TcpStream, Value, Write, fs, orchestrator,
	thread,
};
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
