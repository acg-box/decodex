use crate::orchestrator::tests::operator::status::http::{
	self, Arc, DashboardEventHub, Mutex, OperatorControlRequests, ProjectRegistration,
	PublishedOperatorSnapshot, StateStore, TcpListener, Write as _, orchestrator, thread,
	websocket,
};

#[test]
fn operator_dashboard_websocket_rejects_lane_mutation_controls() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let issue = http::sample_issue("Todo", &[]);
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-1", "In Progress")
		.expect("lease should record");

	let server = thread::spawn(move || {
		let (stream, _) = listener.accept().expect("listener should accept a connection");

		orchestrator::handle_operator_state_endpoint_connection(
			stream,
			&server_snapshot,
			&server_dashboard_events,
			&OperatorControlRequests::default(),
			&server_state_store,
		)
		.expect("websocket handler should complete after client disconnect");
	});
	let (mut client, response, mut frame) = http::open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	for (request_id, action) in [
		("pause-1", "pauseProject"),
		("resume-1", "resumeProject"),
		("interrupt-1", "interruptRun"),
	] {
		let control_message = format!(
			r#"{{"type":"control","requestId":"{request_id}","action":"{action}","projectId":"{}","issueId":"{}","runId":"run-1"}}"#,
			config.service_id(),
			issue.id,
		);

		client
			.write_all(&websocket::websocket_client_text_frame(&control_message))
			.expect("client should send unsupported dashboard control");

		let ack = http::read_websocket_json_until(&mut client, &mut frame, |payload| {
			payload["type"] == "controlAck" && payload["payload"]["requestId"] == request_id
		});

		assert_eq!(ack["payload"]["accepted"], false);
		assert_eq!(ack["payload"]["status"], "unsupported_action");
		assert_eq!(ack["payload"]["action"], action);
		assert_eq!(ack["payload"]["projectId"], config.service_id());
		assert_eq!(ack["payload"]["issueId"], issue.id);
		assert_eq!(ack["payload"]["runId"], "run-1");
	}

	assert!(
		state_store
			.list_projects()
			.expect("projects should list")
			.into_iter()
			.find(|project| project.service_id() == config.service_id())
			.expect("project should remain registered")
			.enabled(),
		"unsupported project controls should not pause dispatch"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run lookup should succeed")
			.expect("run should remain recorded")
			.status(),
		"running",
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_some(),
		"unsupported interrupt should not release the queue lease"
	);

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}
