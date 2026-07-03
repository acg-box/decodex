use crate::orchestrator::tests::operator::status::http::{
	self, Arc, CodexAccountActivitySummary, CodexAccountMarker, DashboardEventHub, Instant, Mutex,
	OPERATOR_DASHBOARD_TEST_TIMEOUT, OperatorControlRequests, Path, ProjectRegistration,
	ProtocolActivityMarker, ProtocolActivitySummary, PublishedOperatorSnapshot, Read as _,
	SocketAddr, StateStore, TcpListener, TcpStream, TempDir, TestEnvVarGuard, Value, Write as _,
	fs, orchestrator, runtime, slice, state, thread,
};
pub(super) fn websocket_text_payload(frame: &[u8]) -> Option<(&[u8], usize)> {
	if frame.len() < 2 || frame[0] != 0x81 {
		return None;
	}

	let payload_length_marker = frame[1] & 0x7f;
	let (payload_offset, payload_length): (usize, usize) = match payload_length_marker {
		length @ 0..=125 => (2_usize, usize::from(length)),
		126 => {
			if frame.len() < 4 {
				return None;
			}

			(4_usize, usize::from(u16::from_be_bytes([frame[2], frame[3]])))
		},
		127 => {
			if frame.len() < 10 {
				return None;
			}

			let length = u64::from_be_bytes([
				frame[2], frame[3], frame[4], frame[5], frame[6], frame[7], frame[8], frame[9],
			]);
			let Ok(length) = usize::try_from(length) else {
				return None;
			};

			(10_usize, length)
		},
		_ => return None,
	};
	let payload_end = payload_offset.checked_add(payload_length)?;

	(frame.len() >= payload_end).then(|| (&frame[payload_offset..payload_end], payload_end))
}

pub(super) fn open_dashboard_websocket_client(address: SocketAddr) -> (TcpStream, String, Vec<u8>) {
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut bytes = Vec::new();
	let mut buffer = [0_u8; 2_048];

	client
		.set_read_timeout(Some(OPERATOR_DASHBOARD_TEST_TIMEOUT))
		.expect("client timeout should configure");
	client
		.write_all(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_WS_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("client should write request");

	let header_end = loop {
		let header_bytes = client.read(&mut buffer).expect("client should read stream headers");

		bytes.extend_from_slice(&buffer[..header_bytes]);

		if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
			break index + 4;
		}
	};
	let response =
		String::from_utf8(bytes[..header_end].to_vec()).expect("headers should be utf-8");
	let frame = bytes[header_end..].to_vec();

	(client, response, frame)
}

pub(super) fn read_websocket_json_until(
	client: &mut TcpStream,
	frame: &mut Vec<u8>,
	matches: impl Fn(&Value) -> bool,
) -> Value {
	let deadline = Instant::now() + OPERATOR_DASHBOARD_TEST_TIMEOUT;
	let mut buffer = [0_u8; 2_048];

	loop {
		assert!(Instant::now() < deadline, "websocket should send expected event");

		if frame.is_empty() {
			let event_bytes = client.read(&mut buffer).expect("client should read websocket event");

			frame.extend_from_slice(&buffer[..event_bytes]);
		}

		if let Some((payload, consumed)) = websocket_text_payload(frame) {
			let payload: Value =
				serde_json::from_slice(payload).expect("event payload should be json");

			frame.drain(..consumed);

			if matches(&payload) {
				return payload;
			}
		} else {
			let event_bytes =
				client.read(&mut buffer).expect("client should continue websocket event");

			frame.extend_from_slice(&buffer[..event_bytes]);
		}
	}
}

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
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);
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

		if let Some((payload, consumed)) = websocket_text_payload(&frame) {
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
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	let initial_snapshot =
		read_websocket_json_until(&mut client, &mut frame, |payload| payload["type"] == "snapshot");

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
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	let _initial_snapshot =
		read_websocket_json_until(&mut client, &mut frame, |payload| payload["type"] == "snapshot");
	let activity = read_websocket_json_until(&mut client, &mut frame, |payload| {
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

#[test]
fn operator_dashboard_websocket_accepts_subscription_and_account_selection_controls() {
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
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();

	state_store.upsert_project(&registration).expect("project should register");

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
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	let control_ready = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlReady"
	});
	let supported_actions = control_ready["payload"]["supportedActions"]
		.as_array()
		.expect("supported actions should list");

	assert!(supported_actions.iter().any(|action| action.as_str() == Some("subscribe")));
	assert!(supported_actions.iter().any(|action| action.as_str() == Some("focus")));
	assert!(supported_actions.iter().any(|action| action.as_str() == Some("clearFocus")));
	assert!(supported_actions.iter().any(|action| action.as_str() == Some("selectAccount")));
	assert!(
		supported_actions.iter().any(|action| action.as_str() == Some("clearAccountSelection"))
	);
	assert!(supported_actions.iter().any(|action| action.as_str() == Some("ack")));
	assert!(!supported_actions.iter().any(|action| action.as_str() == Some("pauseProject")));
	assert!(!supported_actions.iter().any(|action| action.as_str() == Some("resumeProject")));
	assert!(!supported_actions.iter().any(|action| action.as_str() == Some("interruptRun")));

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"subscribe","requestId":"sub-1","projectId":"pubfi"}"#,
		))
		.expect("client should send subscribe");

	let subscribe_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "sub-1"
	});

	assert_eq!(subscribe_ack["payload"]["accepted"], true);
	assert_eq!(subscribe_ack["payload"]["subscription"]["projectId"], "pubfi");

	assert_dashboard_account_selection_controls(&mut client, &mut frame, config.repo_root());
	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

fn assert_dashboard_account_selection_controls(
	client: &mut TcpStream,
	frame: &mut Vec<u8>,
	repo_root: &Path,
) {
	let home = repo_root.parent().expect("fixture repo root should have a parent");
	let _home_guard =
		TestEnvVarGuard::set("HOME", home.to_str().expect("fixture home should be UTF-8"));
	let accounts_dir = home.join(".codex/decodex");

	fs::create_dir_all(&accounts_dir).expect("account pool dir should create");
	fs::write(
		accounts_dir.join("accounts.jsonl"),
		r#"{"email":"copy@example.com","tokens":{"access_token":"token","refresh_token":"refresh","account_id":"acct_123456"}}"#,
	)
	.expect("account pool should write");

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"account-1","action":"selectAccount","accountSelector":"copy@example.com"}"#,
		))
		.expect("client should send account selection");

	let account_ack = read_websocket_json_until(client, frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "account-1"
	});

	assert_eq!(account_ack["payload"]["accepted"], true);
	assert_eq!(account_ack["payload"]["status"], "fixed");

	let account_snapshot = read_websocket_json_until(client, frame, |payload| {
		payload["type"] == "snapshot"
			&& payload["payload"]["snapshot"]["account_control"]["mode"] == "fixed"
	});

	assert_eq!(
		account_snapshot["payload"]["snapshot"]["account_control"]["account_selector"],
		"copy@example.com"
	);
	assert_eq!(
		runtime::global_fixed_account_selector().expect("global account selector should read"),
		Some(String::from("copy@example.com"))
	);
	assert!(
		!fs::read_to_string(http::service_config_path(repo_root))
			.expect("project config should remain readable")
			.contains("fixed_account"),
		"account selection should not write project-scoped fixed_account"
	);

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"account-clear","action":"clearAccountSelection"}"#,
		))
		.expect("client should send account clear");

	let account_clear_ack = read_websocket_json_until(client, frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "account-clear"
	});

	assert_eq!(account_clear_ack["payload"]["accepted"], true);
	assert_eq!(account_clear_ack["payload"]["status"], "balanced");

	let account_clear_snapshot = read_websocket_json_until(client, frame, |payload| {
		payload["type"] == "snapshot"
			&& payload["payload"]["snapshot"]["account_control"]["mode"] == "balanced"
	});

	assert_eq!(
		account_clear_snapshot["payload"]["snapshot"]["account_control"]["account_selector"],
		Value::Null
	);
	assert_eq!(
		runtime::global_fixed_account_selector().expect("global account selector should read"),
		None
	);
}

#[test]
fn operator_dashboard_websocket_controls_focus_and_clear_subscription() {
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
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"focus-1","action":"focus","projectId":"pubfi","issueId":"PUB-101","runId":"run-1"}"#,
		))
		.expect("client should send focus");

	let focus_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "focus-1"
	});

	assert_eq!(focus_ack["payload"]["accepted"], true);
	assert_eq!(focus_ack["payload"]["status"], "focused");
	assert_eq!(focus_ack["payload"]["subscription"]["projectId"], "pubfi");
	assert_eq!(focus_ack["payload"]["subscription"]["issueId"], "PUB-101");
	assert_eq!(focus_ack["payload"]["subscription"]["runId"], "run-1");

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"clear-1","action":"clearFocus"}"#,
		))
		.expect("client should send clear focus");

	let clear_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "clear-1"
	});

	assert_eq!(clear_ack["payload"]["accepted"], true);
	assert_eq!(clear_ack["payload"]["status"], "cleared");
	assert_eq!(clear_ack["payload"]["subscription"]["projectId"], Value::Null);
	assert_eq!(clear_ack["payload"]["subscription"]["issueId"], Value::Null);
	assert_eq!(clear_ack["payload"]["subscription"]["runId"], Value::Null);

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_dashboard_websocket_filters_run_activity_by_subscription() {
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
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"subscribe","requestId":"sub-filter","projectId":"pubfi","runId":"run-2"}"#,
		))
		.expect("client should send subscription");

	let _subscribe_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "sub-filter"
	});

	dashboard_events.broadcast(
		"runActivity",
		serde_json::json!({
			"emittedAtUnixEpoch": 1_774_000_010_i64,
			"currentLanes": [
				{ "project_id": "pubfi", "issue_id": "PUB-101", "run_id": "run-1" },
				{ "project_id": "pubfi", "issue_id": "PUB-102", "run_id": "run-2" },
				{ "project_id": "rsnap", "issue_id": "RS-1", "run_id": "run-2" }
			],
			"presentation": {
				"schema": "decodex.operator.presentation/1",
				"current_lane_cards": [
					{
						"id": "run-1",
						"run_id": "run-1",
						"issue_id": "PUB-101",
						"project_id": "pubfi",
						"run": { "project_id": "pubfi", "issue_id": "PUB-101", "run_id": "run-1" }
					},
					{
						"id": "run-2",
						"run_id": "run-2",
						"issue_id": "PUB-102",
						"project_id": "pubfi",
						"run": { "project_id": "pubfi", "issue_id": "PUB-102", "run_id": "run-2" }
					},
					{
						"id": "run-2-rsnap",
						"run_id": "run-2",
						"issue_id": "RS-1",
						"project_id": "rsnap",
						"run": { "project_id": "rsnap", "issue_id": "RS-1", "run_id": "run-2" }
					}
				]
			}
		}),
	);

	let activity = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "runActivity"
	});
	let current_lanes =
		activity["payload"]["currentLanes"].as_array().expect("current lanes should list");

	assert_eq!(current_lanes.len(), 1);
	assert_eq!(current_lanes[0]["project_id"], "pubfi");
	assert_eq!(current_lanes[0]["issue_id"], "PUB-102");
	assert_eq!(current_lanes[0]["run_id"], "run-2");
	assert_eq!(activity["payload"]["currentLanesComplete"], true);
	assert_eq!(activity["payload"]["currentLaneScope"], "filtered");

	let current_lane_cards = activity["payload"]["presentation"]["current_lane_cards"]
		.as_array()
		.expect("filtered current lane cards should list");

	assert_eq!(current_lane_cards.len(), 1);
	assert_eq!(current_lane_cards[0]["issue_id"], "PUB-102");
	assert_eq!(current_lane_cards[0]["run_id"], "run-2");

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

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
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

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
			.write_all(&websocket_client_text_frame(&control_message))
			.expect("client should send unsupported dashboard control");

		let ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
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

fn websocket_client_text_frame(payload: &str) -> Vec<u8> {
	let payload = payload.as_bytes();
	let mask = [0x11_u8, 0x22, 0x33, 0x44];
	let mut frame = Vec::new();

	frame.push(0x81);

	match payload.len() {
		length @ 0..=125 => frame.push(0x80 | length as u8),
		length @ 126..=65_535 => {
			frame.push(0x80 | 126);
			frame.extend_from_slice(&(length as u16).to_be_bytes());
		},
		length => {
			frame.push(0x80 | 127);
			frame.extend_from_slice(&(length as u64).to_be_bytes());
		},
	}

	frame.extend_from_slice(&mask);
	frame.extend(payload.iter().enumerate().map(|(index, byte)| byte ^ mask[index % mask.len()]));

	frame
}

fn assert_protocol_activity_detail_redacted(protocol_activity: &Value) {
	assert_eq!(protocol_activity["recent_events"][0]["detail"], "redacted_sensitive_detail");
	assert!(!protocol_activity.to_string().contains("path=/srv"));
}

fn assert_run_activity_protocol_activity_redacted(data_lane: &Value, fingerprint_lane: &Value) {
	assert_eq!(data_lane["protocol_activity"]["waiting_reason"], "model");

	assert_protocol_activity_detail_redacted(&data_lane["protocol_activity"]);
	assert_protocol_activity_detail_redacted(&fingerprint_lane["protocol_activity"]);
}

fn assert_run_activity_envelope(payload: &Value, data: &Value, fingerprint: &Value) {
	assert_eq!(payload["type"], "runActivity");
	assert_eq!(data["accountControl"]["mode"], "balanced");
	assert_eq!(data["currentLanesComplete"], true);
	assert_eq!(data["currentLaneScope"], "complete");
	assert!(data.get("accounts").is_none());
	assert!(fingerprint.get("emittedAtUnixEpoch").is_none());
	assert_eq!(fingerprint["accountControl"]["mode"], "balanced");
	assert_eq!(fingerprint["currentLanesComplete"], true);
	assert_eq!(fingerprint["currentLaneScope"], "complete");
	assert!(fingerprint.get("accounts").is_none());
	assert_eq!(data["presentation"]["schema"], "decodex.operator.presentation/1");
	assert_eq!(fingerprint["presentation"]["schema"], "decodex.operator.presentation/1");
	assert_eq!(
		data["presentation"]["current_lane_cards"].as_array().map(Vec::len),
		data["currentLanes"].as_array().map(Vec::len)
	);
	assert_eq!(
		fingerprint["presentation"]["current_lane_cards"].as_array().map(Vec::len),
		fingerprint["currentLanes"].as_array().map(Vec::len)
	);
}

fn assert_run_activity_current_lane(data_lane: &Value, fingerprint_lane: &Value) {
	assert_eq!(fingerprint_lane["run_id"], "run-1");
	assert_eq!(fingerprint_lane["project_display_name"], "hack-ink/pubfi-mono-v2");
	assert_eq!(data_lane["run_id"], "run-1");
	assert_eq!(data_lane["project_id"], "pubfi");
	assert_eq!(data_lane["project_display_name"], "hack-ink/pubfi-mono-v2");

	assert_run_activity_protocol_activity_redacted(data_lane, fingerprint_lane);

	assert_eq!(data_lane["account"]["account_fingerprint"], "acct-1");
	assert_eq!(data_lane["accounts"][0]["account_fingerprint"], "acct-1");
	assert!(data_lane.get("idle_for_seconds").is_some());
	assert!(data_lane.get("protocol_idle_for_seconds").is_some());
	assert!(fingerprint_lane.get("idle_for_seconds").is_none());
	assert!(fingerprint_lane.get("protocol_idle_for_seconds").is_none());
}

#[test]
fn operator_dashboard_run_activity_event_summarizes_current_lanes() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = http::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model")),
		rate_limit_status: None,
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("item/model/delta"),
			category: String::from("model"),
			detail: Some(String::from("state marker path=/srv/decodex/runtime")),
		}],
	};
	let account = CodexAccountActivitySummary {
		account_fingerprint: String::from("acct-1"),
		status: String::from("available"),
		refresh_status: String::from("ok"),
		..Default::default()
	};

	http::git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

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

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 7,
			last_event_type: "item/model/delta",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol activity marker should write");
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

	let event =
		orchestrator::build_operator_run_activity_event(&state_store).expect("event should build");
	let message =
		orchestrator::dashboard_websocket_message(event.event.event_type, &event.event.payload)
			.expect("event should serialize");
	let (payload, _consumed) =
		websocket_text_payload(&message).expect("event should be a text frame");
	let payload: Value = serde_json::from_slice(payload).expect("event data should be json");
	let data = &payload["payload"];
	let fingerprint: Value =
		serde_json::from_slice(&event.fingerprint).expect("fingerprint should be json");

	assert_run_activity_envelope(&payload, data, &fingerprint);
	assert_run_activity_current_lane(&data["currentLanes"][0], &fingerprint["currentLanes"][0]);

	assert_eq!(data["presentation"]["current_lane_cards"][0]["run_id"], "run-1");
	assert_eq!(data["presentation"]["current_lane_cards"][0]["title"], "PUB-101");
	assert_eq!(
		data["presentation"]["current_lane_cards"][0]["assigned_account_fingerprints"][0],
		"acct-1"
	);
	assert_eq!(data["presentation"]["current_lane_cards"][0]["tone"], "waiting");
	assert_eq!(data["presentation"]["current_lane_cards"][0]["counts_as_running"], true);
	assert_eq!(data["presentation"]["current_lane_cards"][0]["is_waiting"], true);
	assert_eq!(fingerprint["presentation"]["current_lane_cards"][0]["run"]["run_id"], "run-1");
}
