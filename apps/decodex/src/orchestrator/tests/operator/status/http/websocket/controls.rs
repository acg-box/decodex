use crate::orchestrator::tests::operator::status::http::{
	self, Arc, DashboardEventHub, Mutex, OperatorControlRequests, Path, ProjectRegistration,
	PublishedOperatorSnapshot, StateStore, TcpListener, TcpStream, TestEnvVarGuard, Value,
	Write as _, fs, orchestrator, runtime, thread,
};

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
	let (mut client, response, mut frame) = super::open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	let control_ready = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
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
		.write_all(&super::websocket_client_text_frame(
			r#"{"type":"subscribe","requestId":"sub-1","projectId":"pubfi"}"#,
		))
		.expect("client should send subscribe");

	let subscribe_ack = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
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
		.write_all(&super::websocket_client_text_frame(
			r#"{"type":"control","requestId":"account-1","action":"selectAccount","accountSelector":"copy@example.com"}"#,
		))
		.expect("client should send account selection");

	let account_ack = super::read_websocket_json_until(client, frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "account-1"
	});

	assert_eq!(account_ack["payload"]["accepted"], true);
	assert_eq!(account_ack["payload"]["status"], "fixed");

	let account_snapshot = super::read_websocket_json_until(client, frame, |payload| {
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
		.write_all(&super::websocket_client_text_frame(
			r#"{"type":"control","requestId":"account-clear","action":"clearAccountSelection"}"#,
		))
		.expect("client should send account clear");

	let account_clear_ack = super::read_websocket_json_until(client, frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "account-clear"
	});

	assert_eq!(account_clear_ack["payload"]["accepted"], true);
	assert_eq!(account_clear_ack["payload"]["status"], "balanced");

	let account_clear_snapshot = super::read_websocket_json_until(client, frame, |payload| {
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
	let (mut client, response, mut frame) = super::open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	client
		.write_all(&super::websocket_client_text_frame(
			r#"{"type":"control","requestId":"focus-1","action":"focus","projectId":"pubfi","issueId":"PUB-101","runId":"run-1"}"#,
		))
		.expect("client should send focus");

	let focus_ack = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "focus-1"
	});

	assert_eq!(focus_ack["payload"]["accepted"], true);
	assert_eq!(focus_ack["payload"]["status"], "focused");
	assert_eq!(focus_ack["payload"]["subscription"]["projectId"], "pubfi");
	assert_eq!(focus_ack["payload"]["subscription"]["issueId"], "PUB-101");
	assert_eq!(focus_ack["payload"]["subscription"]["runId"], "run-1");

	client
		.write_all(&super::websocket_client_text_frame(
			r#"{"type":"control","requestId":"clear-1","action":"clearFocus"}"#,
		))
		.expect("client should send clear focus");

	let clear_ack = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
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
