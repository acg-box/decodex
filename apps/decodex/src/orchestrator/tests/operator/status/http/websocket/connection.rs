use crate::orchestrator::tests::operator::status::http::{
	self, Arc, CodexAccountActivitySummary, CodexAccountMarker, DashboardEventHub, Instant, Mutex,
	OPERATOR_DASHBOARD_TEST_TIMEOUT, OperatorControlRequests, ProjectRegistration,
	PublishedOperatorSnapshot, Read as _, StateStore, TcpListener, Value, orchestrator, slice,
	state, thread,
};
#[test]
fn operator_dashboard_websocket_pushes_broadcast_events() {
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
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
	let (mut client, response, mut frame) = super::open_dashboard_websocket_client(address);
	let mut buffer = [0_u8; 2_048];

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
	assert!(response.contains("Upgrade: websocket"));
	assert!(response.contains("Connection: Upgrade"));
	assert!(response.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));

	dashboard_events.broadcast(
		"snapshot",
		serde_json::json!({
			"snapshotPublishedAtUnixEpoch": 1_774_000_000_i64,
			"snapshot": { "project_id": "pubfi" },
		}),
	);

	let deadline = Instant::now() + OPERATOR_DASHBOARD_TEST_TIMEOUT;
	let payload = loop {
		assert!(Instant::now() < deadline, "websocket should send broadcast events");

		if frame.is_empty() {
			let event_bytes = client.read(&mut buffer).expect("client should read broadcast event");

			frame.extend_from_slice(&buffer[..event_bytes]);
		}

		if let Some((payload, consumed)) = super::websocket_text_payload(&frame) {
			let payload: Value =
				serde_json::from_slice(payload).expect("event payload should be json");

			frame.drain(..consumed);

			if payload["type"] == "snapshot" {
				break payload;
			}
		}
	};

	assert_eq!(payload["type"], "snapshot");
	assert_eq!(payload["payload"]["snapshot"]["project_id"], "pubfi");

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_dashboard_websocket_sends_current_snapshot_on_connect() {
	const SNAPSHOT_UNIX_EPOCH: i64 = 1_774_000_000;

	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(br#"{"project_id":"pubfi","current_lanes":[]}"#.to_vec()),
		last_publish_unix_epoch: Some(SNAPSHOT_UNIX_EPOCH),
	}));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
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
	let (mut client, response, mut frame) = super::open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	let initial_snapshot = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "snapshot"
	});

	assert_eq!(initial_snapshot["payload"]["snapshotPublishedAtUnixEpoch"], SNAPSHOT_UNIX_EPOCH);
	assert_eq!(initial_snapshot["payload"]["snapshot"]["project_id"], "pubfi");

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_dashboard_websocket_sends_cached_run_activity_on_connect() {
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(
			br#"{"project_id":"pubfi","current_lanes":[],"account_control":{"mode":"balanced","account_selector":null}}"#.to_vec(),
		),
		last_publish_unix_epoch: Some(1_774_000_000),
	}));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let account = CodexAccountActivitySummary {
		account_fingerprint: String::from("acct-1"),
		status: String::from("available"),
		refresh_status: String::from("ok"),
		..Default::default()
	};
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_account_marker(
		&worktree_path,
		&CodexAccountMarker {
			run_id: "run-1",
			attempt_number: 1,
			account: &account,
			accounts: slice::from_ref(&account),
		},
	)
	.expect("account marker should write");

	let run_activity = orchestrator::build_operator_run_activity_event(&state_store)
		.expect("run activity should build");

	dashboard_events.broadcast(run_activity.event.event_type, run_activity.event.payload);

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
	let (mut client, response, mut frame) = super::open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	let _initial_snapshot = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "snapshot"
	});
	let activity = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "runActivity"
	});

	assert_eq!(activity["payload"]["currentLanes"][0]["run_id"], "run-1");
	assert_eq!(activity["payload"]["currentLanes"][0]["account"]["account_fingerprint"], "acct-1");
	assert_eq!(
		activity["payload"]["currentLanes"][0]["accounts"][0]["account_fingerprint"],
		"acct-1"
	);

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}
