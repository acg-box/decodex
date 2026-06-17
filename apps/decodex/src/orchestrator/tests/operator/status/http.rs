use std::io::ErrorKind;
use std::net::SocketAddr;

use orchestrator::OperatorControlRequests;
use state::RUN_CONTROL_CHANNEL_DIR;
use state::RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE;
use process::Child;
use orchestrator::DASHBOARD_MAX_WEBSOCKET_CLIENTS;
use orchestrator::DashboardClientSubscription;

use crate::runtime;

const OPERATOR_DASHBOARD_TEST_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(unix)]
struct RunLeaseMissingControlFixture {
	issue: TrackerIssue,
	channel_path: PathBuf,
	child: Child,
	child_process_id: u32,
}
#[cfg(unix)]
impl Drop for RunLeaseMissingControlFixture {
	fn drop(&mut self) {
		if matches!(self.child.try_wait(), Ok(None)) {
			let _ = self.child.kill();
			let _ = self.child.wait();
		}
	}
}

#[test]
fn operator_state_endpoint_serves_dashboard_html_from_root_and_dashboard_route() {
	for path in [
		OPERATOR_DASHBOARD_ENDPOINT_PATH,
		OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH,
	] {
		let response = String::from_utf8(
			orchestrator::build_operator_state_http_response(
				format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
					.as_bytes(),
			)
			.expect("dashboard response should build"),
		)
		.expect("dashboard response should be utf-8");

		assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
		assert!(response.contains("Content-Type: text/html; charset=utf-8"));
		assert!(response.contains("<title>Decodex</title>"));
		assert!(response.contains("<h1 id=\"project-title\">Decodex</h1>"));
		assert!(response.contains("Delivery flow"));
		assert!(response.contains("flow-queue"));
		assert!(response.contains("<span>Intake</span>"));
		assert!(response.contains("<span>Landing</span>"));
		assert!(response.contains("section-marker section-marker-projects"));
		assert!(!response.contains("<h2 id=\"projects-title\">Projects</h2>"));
		assert!(!response.contains("data-fold-key=\"panel:projects\""));
		assert!(response.contains("id=\"project-filter-toggle\""));
		assert!(response.contains("class=\"project-table\" role=\"table\""));
		assert!(!response.contains("<h2>All</h2>"));
		assert!(response.contains("projectRegistrationCommand"));
		assert!(
			response.contains("decodex project add ~/.codex/decodex/projects/<service-id>")
		);
		assert!(response.contains("Register projects explicitly"));
		assert!(response.contains("does not scan history or repos"));
		assert!(response.contains("data-detail-key"));
		assert!(response.contains("notice-dock"));
		assert!(response.contains("Notices"));
		assert!(response.contains("notice-panel"));
			assert!(response.contains("Snapshot stream"));
		assert!(response.contains("Snapshot warning"));
		assert!(response.contains("Tracker sync paused"));
		assert!(response.contains("connector_backoffs"));
		assert!(response.contains("Sync backoff"));
		assert!(response.contains("project_id"));
		assert!(response.contains("retry_after_seconds"));
		assert!(response.contains("reset_at"));
		assert!(response.contains("sync_phase"));
		assert!(!response.contains("error-banner"));
		assert!(!response.contains("metric-active"));
		assert!(!response.contains("Queued issue -> reviewed change -> landed branch"));
		assert!(response.contains("Current Lanes"));
		assert!(response.contains("Intake Queue"));
		assert!(!response.contains("At capacity"));
		assert!(response.contains("Review &amp; Landing"));
		assert!(response.contains("Run History"));
		assert!(response.contains("historyLedgerOutcome"));
		assert!(response.contains("Run history unavailable"));
		assert!(response.contains("renderHistoryLedgerFacts"));
		assert!(response.contains("Recovery Worktrees"));
		assert!(response.contains("Lane activity"));
		assert!(response.contains("agent idle"));
		assert!(response.contains("Child agent"));
		assert!(response.contains("<span>Activity</span>"));
		assert!(!response.contains("<span>Agent Now</span>"));
			assert!(response.contains("current window"));
			assert!(response.contains("peak window"));
			assert!(!response.contains("same as current"));
			assert!(response.contains("Context lifecycle metrics"));
				assert!(response.contains("rows.push({ label: \"Context\", segments: contextSegments });"));
				assert!(response.contains("renderChildLifecyclePhaseTable(lifecycle.phases || [])"));
			assert!(response.contains("facts.push([\"tokens\", tokenSummary]);"));
			assert!(!response.contains("\"cumulative input\","));
			assert!(response.contains("Current context window from the latest child-agent event."));
			assert!(response.contains("child_agent_activity"));
		assert!(response.contains("renderChildAgentBreakdown"));
		assert!(response.contains("Debug Details"));
		assert!(response.contains("already running"));
		assert!(!response.contains("running laness"));
		assert!(!response.contains("active-echo"));
		assert!(response.contains("fold-panel"));
		assert!(response.contains(".fold-indicator::before"));
		assert!(response.contains("content: \"+\";"));
		assert!(response.contains("content: \"-\";"));
		assert!(!response.contains(".fold-indicator::after"));
		assert!(response.contains("data-fold-key=\"panel:worktrees\""));
		assert!(response.contains("data-fold-key=\"panel:recent\""));
		assert!(response.contains("cursor: pointer;"));
		assert!(response.contains("animateDetail(details, !details.open)"));
		assert!(response.contains("width: min(380px, calc(100vw - 36px));"));
		assert!(response.contains(".notice-item p"));
		assert!(response.contains("font-size: var(--type-body);"));
		assert!(!response.contains(".fold-panel.is-empty .fold-indicator"));
		assert!(!response.contains("details.classList.contains(\"is-empty\")"));
		assert!(!response.contains("Operator views"));
		assert!(!response.contains("Command Brief"));
		assert!(!response.contains("Intake Pressure"));
		assert!(!response.contains("Landing Readiness"));

	assert_dashboard_html_control_surface(response.as_str());

		assert!(!response.contains("Last updated: none"));
		assert!(!response.contains("Auto-refresh"));
		assert!(!response.contains("<h2>Project Scope</h2>"));
		assert!(!response.contains("Projects appear on the first state update"));
		assert!(!response.contains("Diagnostics"));
		assert!(!response.contains("State JSON"));
		assert!(!response.contains("Ready probe"));
		assert!(!response.contains("Live probe"));
		assert!(!response.contains("/livez"));
	}
}

fn assert_dashboard_html_control_surface(response: &str) {
	for required in [
		"/dashboard/control",
		"WebSocket",
		"applyDashboardRunActivity",
		"sendDashboardControl",
		"controlAck",
	] {
		assert!(response.contains(required), "missing required dashboard control marker `{required}`");
	}
	for forbidden in [
		"/state",
		"/readyz",
		"data-dashboard-control=\"interruptRun\"",
		"aria-label=\"Stop this active Decodex work\"",
		"runInterruptControlEnabled",
		"renderRunStopControl",
		"action === \"interruptRun\"",
		"case \"interruptRun\"",
		"data-dashboard-control=\"focusProject\"",
		"data-dashboard-control=\"focusRun\"",
		"data-dashboard-control=\"pauseProject\"",
		"data-dashboard-control=\"resumeProject\"",
		"data-dashboard-control=\"retryRun\"",
		">Retry now</button>",
		">Retry</button>",
		"run.wait_reason),",
	] {
		assert!(
			!response.contains(forbidden),
			"unexpected dashboard control marker `{forbidden}`"
		);
	}
}

#[test]
fn operator_dashboard_uses_decodex_brand_icons() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("dashboard response should build"),
	)
	.expect("dashboard response should be utf-8");

	assert!(response.contains(r#"<link rel="icon" type="image/png" href="/assets/icon.png" />"#));
	assert!(response.contains(r#"<link rel="icon" href="/assets/logo.ico" />"#));
	assert!(
		response.contains(r#"<link rel="apple-touch-icon" sizes="180x180" href="/assets/logo-touch.png" />"#)
	);
	assert!(!response.contains("data:image/svg+xml"));
	assert!(!response.contains("M18 57V23"));
}

#[test]
fn operator_state_endpoint_serves_decodex_brand_assets() {
	for (path, content_type, signature) in [
		("/assets/icon.png", "image/png", b"\x89PNG\r\n\x1a\n".as_slice()),
		("/assets/logo-touch.png", "image/png", b"\x89PNG\r\n\x1a\n".as_slice()),
		("/assets/logo.ico", "image/x-icon", b"\0\0\x01\0".as_slice()),
	] {
		let response = orchestrator::build_operator_state_http_response(
			format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
				.as_bytes(),
		)
		.expect("asset response should build");
		let header_end = response
			.windows(4)
			.position(|window| window == b"\r\n\r\n")
			.expect("response should contain headers");
		let headers = String::from_utf8(response[..header_end].to_vec())
			.expect("headers should be utf-8");
		let body = &response[(header_end + 4)..];

		assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
		assert!(headers.contains(&format!("Content-Type: {content_type}")));
		assert!(body.starts_with(signature));
	}
}

#[test]
fn operator_state_endpoint_rejects_dashboard_websocket_without_upgrade() {
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_WS_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("dashboard websocket response should build"),
	)
	.expect("dashboard websocket response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 426 Upgrade Required\r\n"));
	assert!(response.contains("Upgrade: websocket"));
	assert!(response.ends_with("websocket upgrade required"));
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

	let initial_snapshot = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "snapshot"
	});

	assert_eq!(
		initial_snapshot["payload"]["snapshotPublishedAtUnixEpoch"],
		SNAPSHOT_UNIX_EPOCH
	);
	assert_eq!(initial_snapshot["payload"]["snapshot"]["project_id"], "pubfi");

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_dashboard_websocket_sends_cached_run_activity_on_connect() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("Todo", &[]);
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

	let run_activity =
		orchestrator::build_operator_run_activity_event(&state_store).expect("run activity should build");

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

	let _initial_snapshot = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "snapshot"
	});
	let activity = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "runActivity"
	});

	assert_eq!(activity["payload"]["currentLanes"][0]["run_id"], "run-1");
	assert_eq!(
		activity["payload"]["currentLanes"][0]["account"]["account_fingerprint"],
		"acct-1"
	);
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
		&service_config_path(config.repo_root()),
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
		supported_actions
			.iter()
			.any(|action| action.as_str() == Some("clearAccountSelection"))
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
	let _home_guard = TestEnvVarGuard::set("HOME", home.to_str().expect("fixture home should be UTF-8"));
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
		!fs::read_to_string(service_config_path(repo_root))
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
			]
		}),
	);

	let activity = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "runActivity"
	});
	let current_lanes = activity["payload"]["currentLanes"]
		.as_array()
		.expect("current lanes should list");

	assert_eq!(current_lanes.len(), 1);
	assert_eq!(current_lanes[0]["project_id"], "pubfi");
	assert_eq!(current_lanes[0]["issue_id"], "PUB-102");
	assert_eq!(current_lanes[0]["run_id"], "run-2");
	assert_eq!(activity["payload"]["currentLanesComplete"], false);
	assert_eq!(activity["payload"]["currentLaneScope"], "filtered");

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_dashboard_websocket_rejects_lane_mutation_controls() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let issue = sample_issue("Todo", &[]);
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
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
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.is_some(),
		"unsupported interrupt should not release the queue lease"
	);

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

fn websocket_text_payload(frame: &[u8]) -> Option<(&[u8], usize)> {
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

fn open_dashboard_websocket_client(address: SocketAddr) -> (TcpStream, String, Vec<u8>) {
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
	let response = String::from_utf8(bytes[..header_end].to_vec())
		.expect("headers should be utf-8");
	let frame = bytes[header_end..].to_vec();

	(client, response, frame)
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

fn read_websocket_json_until(
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
			let event_bytes = client.read(&mut buffer).expect("client should continue websocket event");

			frame.extend_from_slice(&buffer[..event_bytes]);
		}
	}
}

#[test]
fn operator_dashboard_run_activity_event_summarizes_current_lanes() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model")),
		rate_limit_status: None,
		recent_events: vec![state::ProtocolActivityEventSummary {
			event_type: String::from("item/model/delta"),
			category: String::from("model"),
			detail: Some(String::from("received model delta")),
		}],
	};
	let account = CodexAccountActivitySummary {
		account_fingerprint: String::from("acct-1"),
		status: String::from("available"),
		refresh_status: String::from("ok"),
		..Default::default()
	};

	git_status_success(
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
	let message = orchestrator::dashboard_websocket_message(
		event.event.event_type,
		&event.event.payload,
	)
	.expect("event should serialize");
	let (payload, _consumed) = websocket_text_payload(&message).expect("event should be a text frame");
	let payload: Value = serde_json::from_slice(payload).expect("event data should be json");
	let data = &payload["payload"];
	let fingerprint: Value =
		serde_json::from_slice(&event.fingerprint).expect("fingerprint should be json");

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
	assert_eq!(fingerprint["currentLanes"][0]["run_id"], "run-1");
	assert_eq!(
		fingerprint["currentLanes"][0]["project_display_name"],
		"hack-ink/pubfi-mono-v2"
	);
	assert_eq!(data["currentLanes"][0]["run_id"], "run-1");
	assert_eq!(data["currentLanes"][0]["project_id"], "pubfi");
	assert_eq!(data["currentLanes"][0]["project_display_name"], "hack-ink/pubfi-mono-v2");
	assert_eq!(data["currentLanes"][0]["protocol_activity"]["waiting_reason"], "model");
	assert_eq!(data["currentLanes"][0]["account"]["account_fingerprint"], "acct-1");
	assert_eq!(data["currentLanes"][0]["accounts"][0]["account_fingerprint"], "acct-1");
	assert!(data["currentLanes"][0].get("idle_for_seconds").is_some());
	assert!(data["currentLanes"][0].get("protocol_idle_for_seconds").is_some());
	assert!(fingerprint["currentLanes"][0].get("idle_for_seconds").is_none());
	assert!(fingerprint["currentLanes"][0].get("protocol_idle_for_seconds").is_none());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_dashboard_run_activity_event_keeps_unleased_app_server_current_lane() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[],
	)
	.expect("thread status should write");

	rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let event =
		orchestrator::build_operator_run_activity_event(&state_store).expect("event should build");
	let message = orchestrator::dashboard_websocket_message(
		event.event.event_type,
		&event.event.payload,
	)
	.expect("event should serialize");
	let (payload, _consumed) = websocket_text_payload(&message).expect("event should be a text frame");
	let payload: Value = serde_json::from_slice(payload).expect("event data should be json");
	let data = &payload["payload"];
	let current_lanes = data["currentLanes"].as_array().expect("current lanes should list");

	assert_eq!(payload["type"], "runActivity");
	assert_eq!(data["currentLanesComplete"], true);
	assert_eq!(current_lanes.len(), 1);
	assert_eq!(current_lanes[0]["run_id"], "run-1");
	assert_eq!(current_lanes[0]["project_id"], "pubfi");
	assert_eq!(current_lanes[0]["project_display_name"], "hack-ink/pubfi-mono-v2");
	assert_eq!(current_lanes[0]["run_lease"], false);
	assert_eq!(current_lanes[0]["execution_liveness"], "process_identity_mismatch");
	assert_eq!(current_lanes[0]["process_alive"], false);
	assert_eq!(current_lanes[0]["process_liveness_reason"], "host_boot_id_mismatch");
	assert_eq!(current_lanes[0]["thread_status"], "active");
}

#[test]
fn operator_dashboard_run_activity_event_demotes_cleanup_complete_unleased_current_lane() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue_with_sort_fields(
		"issue-xy-952",
		"XY-952",
		"Done",
		&[],
		Some(3),
		"2026-06-16T08:50:00Z",
	);
	let worktree_path = config.worktree_root().join("XY-952");

	git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("xy-952-attempt-2-1781598614", &issue.id, 2, "running")
		.expect("stale running attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"y/elf-xy-952",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_event(
			"xy-952-attempt-2-1781598614",
			1,
			"item/tool/call",
			"{\"tool\":\"issue_progress_checkpoint\"}",
		)
		.expect("protocol evidence should record");

	seed_local_linear_execution_events(
		&state_store,
		&successful_linear_execution_history_comments_with_cleanup(&issue),
	);

	let event =
		orchestrator::build_operator_run_activity_event(&state_store).expect("event should build");
	let message = orchestrator::dashboard_websocket_message(
		event.event.event_type,
		&event.event.payload,
	)
	.expect("event should serialize");
	let (payload, _consumed) = websocket_text_payload(&message).expect("event should be a text frame");
	let payload: Value = serde_json::from_slice(payload).expect("event data should be json");
	let data = &payload["payload"];
	let fingerprint: Value =
		serde_json::from_slice(&event.fingerprint).expect("fingerprint should be json");

	assert_eq!(payload["type"], "runActivity");
	assert_eq!(data["currentLanesComplete"], true);
	assert_eq!(data["currentLaneScope"], "complete");
	assert_eq!(data["currentLanes"].as_array().map(Vec::len), Some(0));
	assert_eq!(fingerprint["currentLanes"].as_array().map(Vec::len), Some(0));
	assert!(!data.to_string().contains("xy-952-attempt-2-1781598614"));
}

#[test]
fn operator_dashboard_run_activity_fingerprint_ignores_volatile_timing_fields() {
	let mut first = serde_json::json!({
		"accountControl": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"currentLanes": [
			{
				"run_id": "run-1",
				"status": "running",
				"phase": "executing",
				"idle_for_seconds": 4,
				"protocol_idle_for_seconds": 3,
				"child_agent_activity": {
					"current_bucket": "model",
					"current_elapsed_seconds": 2,
					"buckets": [
						{
							"bucket": "model",
							"wall_seconds": 2,
							"event_count": 7,
						},
					],
				},
			},
		],
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
	});
	let mut second = serde_json::json!({
		"accountControl": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"currentLanes": [
			{
				"run_id": "run-1",
				"status": "running",
				"phase": "executing",
				"idle_for_seconds": 5,
				"protocol_idle_for_seconds": 4,
				"child_agent_activity": {
					"current_bucket": "model",
					"current_elapsed_seconds": 3,
					"buckets": [
						{
							"bucket": "model",
							"wall_seconds": 3,
							"event_count": 7,
						},
					],
				},
			},
		],
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
	});

	orchestrator::strip_dashboard_run_activity_volatile_fields(&mut first);
	orchestrator::strip_dashboard_run_activity_volatile_fields(&mut second);

	assert_eq!(first, second);
	assert_eq!(first["currentLanes"][0]["run_id"], "run-1");
	assert_eq!(first["currentLanes"][0]["child_agent_activity"]["buckets"][0]["event_count"], 7);
	assert!(first["currentLanes"][0].get("idle_for_seconds").is_none());
	assert!(
		first["currentLanes"][0]["child_agent_activity"]
			.get("current_elapsed_seconds")
			.is_none()
	);
	assert!(
		first["currentLanes"][0]["child_agent_activity"]["buckets"][0]
			.get("wall_seconds")
			.is_none()
	);
}

#[test]
fn dashboard_event_hub_unregisters_websocket_clients_and_caps_fanout() {
	let hub = DashboardEventHub::default();
	let mut registrations = Vec::new();

	for _ in 0..DASHBOARD_MAX_WEBSOCKET_CLIENTS {
		registrations.push(hub.subscribe().expect("client should subscribe below cap"));
	}

	assert_eq!(
		hub.client_count_for_test(),
		orchestrator::DASHBOARD_MAX_WEBSOCKET_CLIENTS
	);
	assert!(
		hub.subscribe().is_err(),
		"client fanout should be capped instead of growing unbounded"
	);

	drop(registrations.pop());

	assert_eq!(
		hub.client_count_for_test(),
		orchestrator::DASHBOARD_MAX_WEBSOCKET_CLIENTS - 1
	);

	let replacement = hub.subscribe().expect("slot should reopen after client drop");

	assert_eq!(
		hub.client_count_for_test(),
		orchestrator::DASHBOARD_MAX_WEBSOCKET_CLIENTS
	);

	drop(replacement);
	drop(registrations);

	assert_eq!(hub.client_count_for_test(), 0);
}

#[test]
fn dashboard_event_hub_caches_and_filters_last_run_activity_event() {
	let hub = DashboardEventHub::default();
	let payload = serde_json::json!({
		"emittedAtUnixEpoch": 1_774_000_000,
		"accountControl": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"currentLanes": [
			{
				"project_id": "decodex",
				"issue_id": "issue-1",
				"run_id": "run-1",
			},
			{
				"project_id": "decodex",
				"issue_id": "issue-2",
				"run_id": "run-2",
			},
		],
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
	});
	let subscription = DashboardClientSubscription {
		project_id: Some(String::from("decodex")),
		issue_id: Some(String::from("issue-1")),
		run_id: None,
	};

	hub.broadcast("runActivity", payload);
	hub.broadcast("snapshot", serde_json::json!({"ignored": true}));

	let event = hub
		.cached_run_activity_event(&subscription)
		.expect("cached run activity should remain available after other event types");
	let current_lanes = event.payload["currentLanes"]
		.as_array()
		.expect("filtered current lanes should be an array");

	assert_eq!(event.event_type, "runActivity");
	assert_eq!(current_lanes.len(), 1);
	assert_eq!(current_lanes[0]["issue_id"], "issue-1");
	assert_eq!(event.payload["currentLanesComplete"], false);
	assert_eq!(event.payload["currentLaneScope"], "filtered");
}

#[test]
fn operator_dashboard_run_activity_event_includes_disabled_project_current_lanes() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer_store = StateStore::open(&state_path).expect("observer store should open");
	let writer_store = StateStore::open(&state_path).expect("writer store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		false,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

	observer_store.upsert_project(&registration).expect("project should register");
	writer_store
		.record_run_attempt("run-disabled-active", &issue.id, 1, "running")
		.expect("current lane should record");
	writer_store
		.upsert_lease(config.service_id(), &issue.id, "run-disabled-active", "In Progress")
		.expect("run lease should record");
	writer_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let event = orchestrator::build_operator_run_activity_event(&observer_store)
		.expect("event should build");
	let message = orchestrator::dashboard_websocket_message(
		event.event.event_type,
		&event.event.payload,
	)
	.expect("event should serialize");
	let (payload, _consumed) = websocket_text_payload(&message).expect("event should be a text frame");
	let payload: Value = serde_json::from_slice(payload).expect("event data should be json");
	let data = &payload["payload"];
	let current_lanes = data["currentLanes"].as_array().expect("current lanes should list");

	assert_eq!(payload["type"], "runActivity");
	assert_eq!(current_lanes.len(), 1);
	assert_eq!(current_lanes[0]["run_id"], "run-disabled-active");
	assert_eq!(current_lanes[0]["project_id"], "pubfi");
	assert_eq!(current_lanes[0]["project_display_name"], "hack-ink/pubfi-mono-v2");
	assert_eq!(data["currentLanesComplete"], true);
	assert_eq!(data["currentLaneScope"], "complete");
}

#[test]
fn operator_state_endpoint_reads_complete_headers_before_parsing() {
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
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
		.expect("handler should accept segmented headers");
	});
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = String::new();

	client.write_all(b"GET /dash").expect("client should write first request fragment");

	thread::sleep(Duration::from_millis(10));

	client
		.write_all(b"board HTTP/1.1\r\nHost: localhost\r\n\r\n")
		.expect("client should write second request fragment");
	client.shutdown(Shutdown::Write).expect("client should close the request body stream");
	client.read_to_string(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.contains("<title>Decodex</title>"));
}

#[test]
fn operator_state_endpoint_overlays_live_account_control_on_published_snapshot() {
	const SNAPSHOT_UNIX_EPOCH: i64 = 1_774_000_000;

	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let stale_snapshot = serde_json::json!({
		"project_id": "all",
		"account_control": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"current_lanes": [],
		"recent_runs": [],
		"history_lanes": [],
		"queued_candidates": [],
		"worktrees": [],
		"post_review_lanes": [],
	});

	runtime::write_global_fixed_account_selector(Some("copy@example.com"))
		.expect("global account selector should write");

	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(serde_json::to_vec(&stale_snapshot).expect("snapshot should serialize")),
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
		.expect("handler should serve websocket snapshot");
	});
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);
	let served_snapshot = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "snapshot"
	});

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));
	assert_eq!(served_snapshot["payload"]["snapshot"]["account_control"]["mode"], "fixed");
	assert_eq!(
		served_snapshot["payload"]["snapshot"]["account_control"]["account_selector"],
		"copy@example.com"
	);
	assert_eq!(served_snapshot["payload"]["snapshot"]["project_id"], "all");
	assert_eq!(
		served_snapshot["payload"]["snapshotPublishedAtUnixEpoch"],
		SNAPSHOT_UNIX_EPOCH
	);

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_state_endpoint_serves_large_app_snapshot_without_truncation() {
	const SNAPSHOT_UNIX_EPOCH: i64 = 1_774_000_000;

	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let large_title = "x".repeat(2_000_000);
	let snapshot_value = serde_json::json!({
		"project_id": "all",
		"account_control": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"current_lanes": [],
		"recent_runs": [],
		"history_lanes": [{
			"issue_identifier": "PUB-100",
			"title": large_title,
		}],
		"queued_candidates": [],
		"worktrees": [],
		"post_review_lanes": [],
	});
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");

	listener.set_nonblocking(true).expect("listener should be nonblocking");

	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(serde_json::to_vec(&snapshot_value).expect("snapshot should serialize")),
		last_publish_unix_epoch: Some(SNAPSHOT_UNIX_EPOCH),
	}));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
	let server = thread::spawn(move || {
		let stream = loop {
			match listener.accept() {
				Ok((stream, _)) => break stream,
				Err(error) if error.kind() == ErrorKind::WouldBlock => {
					thread::sleep(Duration::from_millis(5));
				},
				Err(error) => panic!("listener should accept a connection: {error}"),
			}
		};

		orchestrator::handle_operator_state_endpoint_connection(
			stream,
			&server_snapshot,
			&server_dashboard_events,
			&OperatorControlRequests::default(),
			&server_state_store,
		)
		.expect("handler should serve the complete large app snapshot");
	});
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = Vec::new();

	client
		.write_all(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("client should write request");
	client.shutdown(Shutdown::Write).expect("client should close request body");

	thread::sleep(Duration::from_millis(350));

	client.read_to_end(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	let header_end = response
		.windows(4)
		.position(|window| window == b"\r\n\r\n")
		.expect("response should contain header terminator");
	let headers = String::from_utf8(response[..header_end].to_vec())
		.expect("response headers should be utf-8");
	let content_length = headers
		.lines()
		.find_map(|line| line.strip_prefix("Content-Length: "))
		.expect("response should contain Content-Length")
		.parse::<usize>()
		.expect("Content-Length should parse");
	let body = &response[header_end + 4..];
	let body_json: Value = serde_json::from_slice(body).expect("body should be complete JSON");

	assert!(headers.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(body.len(), content_length);
	assert_eq!(body_json["history_lanes"][0]["issue_identifier"], "PUB-100");
	assert_eq!(
		body_json["snapshotPublishedAtUnixEpoch"],
		Value::Null,
		"published epoch remains an HTTP header for app snapshot responses"
	);
}

#[test]
fn operator_state_endpoint_livez_ignores_poisoned_snapshot_lock() {
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(br#"{"status":"ok"}"#.to_vec()),
		last_publish_unix_epoch: Some(OffsetDateTime::now_utc().unix_timestamp()),
	}));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let poisoned_snapshot = Arc::clone(&snapshot);
	let _ = panic::catch_unwind(move || {
		let _guard = poisoned_snapshot.lock().expect("snapshot lock should acquire");

		panic!("poison snapshot lock");
	});
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
		.expect("live probe should not require snapshot lock");
	});
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = String::new();

	client
		.write_all(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_LIVE_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("client should write request");
	client.shutdown(Shutdown::Write).expect("client should close the request body stream");
	client.read_to_string(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.ends_with("ok"));
}

#[test]
fn operator_state_endpoint_serves_only_liveness_probe() {
	let live_response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_LIVE_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("live response should build"),
	)
	.expect("live response should be utf-8");

	assert!(live_response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(live_response.ends_with("ok"));
}

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
		orchestrator::build_operator_state_http_response_with_control_requests(
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
		control_requests
			.drain_linear_scan_requests()
			.expect("linear scan requests should drain"),
		vec![orchestrator::OperatorLinearScanRequest {
			project_id: Some(String::from("pubfi")),
		}]
	);
}

#[test]
fn operator_lane_inspect_api_returns_lane_identity() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree should exist");

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("pub-101-attempt-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.update_run_thread("pub-101-attempt-1", "thread-1")
		.expect("thread should record");
	state_store
		.update_run_turn("pub-101-attempt-1", "turn-1")
		.expect("turn should record");
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
	assert_eq!(data["runs"][0]["policyState"], "review_pending");
	assert_eq!(data["runs"][0]["terminalizationState"], "none");
	assert_eq!(
		data["runs"][0]["laneControlNextAction"],
		"Record the independent Decodex Review checkpoint for the current lane head."
	);
	assert_eq!(data["runs"][0]["threadId"], "thread-1");
	assert_eq!(data["runs"][0]["turnId"], "turn-1");
	assert_eq!(data["runs"][0]["softInterruptAvailable"], false);
	assert_eq!(data["runs"][0]["hardInterruptRequiresForce"], true);
}

#[test]
fn operator_lane_inspect_api_filters_by_run_id() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let body = br#"{"projectId":"pubfi","issue":"PUB-101","runId":"pub-101-attempt-1","force":true}"#;
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
	state_store
		.update_run_thread("pub-101-attempt-1", "thread-1")
		.expect("thread should record");
	state_store
		.update_run_turn("pub-101-attempt-1", "turn-1")
		.expect("turn should record");
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
	assert!(data["nextAction"].as_str().unwrap_or_default().contains("Hard fallback was unavailable"));
}

#[test]
fn operator_lane_interrupt_api_force_does_not_hard_fallback_after_control_rejection() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
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
	state_store
		.update_run_thread("pub-101-attempt-1", "thread-1")
		.expect("thread should record");
	state_store
		.update_run_turn("pub-101-attempt-1", "turn-1")
		.expect("turn should record");
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
		&service_config_path(config.repo_root()),
		config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
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
	state_store
		.update_run_thread("pub-101-attempt-1", "thread-1")
		.expect("thread should record");
	state_store
		.update_run_turn("pub-101-attempt-1", "turn-1")
		.expect("turn should record");
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
		.unwrap_or_else(|| panic!("{context} response should include body"));

	serde_json::from_str(body).unwrap_or_else(|_| panic!("{context} response should be json"))
}

#[cfg(unix)]
fn reap_run_lease_missing_child(fixture: &mut RunLeaseMissingControlFixture) {
	if orchestrator::process_is_alive(fixture.child_process_id) {
		fixture
			.child
			.kill()
			.expect("lane child process should be killable after failed fallback");
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
	assert_eq!(
		missing_lease_interrupt_event.payload()["lane"]["run_lease"].as_bool(),
		Some(false)
	);
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
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

	assert!(
		steer_response.starts_with("HTTP/1.1 409 Conflict\r\n"),
		"{steer_response}"
	);
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
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
		child
			.kill()
			.expect("lane child process should be killable after failed fallback");
	}

	child.wait().expect("lane child process should reap");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
	assert_eq!(data["classification"], "hard_interrupt_fallback");
	assert_eq!(data["softInterrupt"]["status"], "unavailable");
	assert_eq!(data["softInterrupt"]["errorClass"], "lane_not_active");
	assert_eq!(data["hardInterrupt"]["classification"], "hard_interrupt_fallback");
	assert_eq!(data["hardInterrupt"]["status"], "sent");
	assert_eq!(
		data["hardInterrupt"]["processId"].as_u64(),
		Some(u64::from(child_process_id))
	);
	assert_eq!(data["hardInterrupt"]["processAliveAfter"], false);
}

#[test]
fn operator_lane_steer_api_rejects_stale_expected_turn_id() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
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
	state_store
		.update_run_thread("pub-101-attempt-1", "thread-1")
		.expect("thread should record");
	state_store
		.update_run_turn("pub-101-attempt-1", "turn-1")
		.expect("turn should record");
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = sample_issue("In Progress", &[]);
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
	state_store
		.update_run_thread("pub-101-attempt-1", "thread-1")
		.expect("thread should record");
	state_store
		.update_run_turn("pub-101-attempt-1", "turn-1")
		.expect("turn should record");
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

#[test]
fn operator_state_endpoint_serves_account_api_snapshot() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			b"GET /api/accounts?refresh=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
		)
		.expect("account response should build"),
	)
	.expect("account response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.contains("Content-Type: application/json"));

	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.expect("account response should include body");
	let data: Value = serde_json::from_str(body).expect("account response should be json");

	assert_eq!(data["accounts"], serde_json::json!([]));
	assert_eq!(data["usage_probe_error"], Value::Null);
	assert!(data["accounts_path"].as_str().is_some_and(|path| {
		path.ends_with(".codex/decodex/accounts.jsonl")
	}));
}

#[test]
fn operator_state_endpoint_persists_account_random_name_offset() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let accounts_dir = temp_dir.path().join(".codex/decodex");
	let accounts_path = accounts_dir.join("accounts.jsonl");

	fs::create_dir_all(&accounts_dir).expect("accounts dir should create");
	fs::write(
		&accounts_path,
		r#"{"email":"copy@example.com","tokens":{"access_token":"token","refresh_token":"refresh","account_id":"acct_123456"}}"#,
	)
	.expect("account pool should write");

	let body = br#"{"selector":"copy@example.com"}"#;
	let request = format!(
		"POST /api/accounts/reroll-name HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
		body.len(),
		String::from_utf8_lossy(body)
	);
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(request.as_bytes())
			.expect("account reroll response should build"),
	)
	.expect("account reroll response should be utf-8");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));

	let data: Value = serde_json::from_str(
		response
			.split_once("\r\n\r\n")
			.map(|(_, body)| body)
			.expect("account reroll response should include body"),
	)
	.expect("account reroll response should be json");

	assert_eq!(data["accounts"][0]["random_name_offset"], 1);
	assert_eq!(data["accounts"][0]["random_name_key"], "df65f796");
	assert_eq!(data["accounts"][0]["random_name"], "Logan");
	assert!(
		fs::read_to_string(accounts_dir.join("config.toml"))
			.expect("global config should read")
			.contains("df65f796 = 1")
	);
}

#[test]
fn operator_state_endpoint_rejects_removed_http_snapshot_routes() {
	for removed_path in ["/state", "/readyz"] {
		let response = String::from_utf8(
			orchestrator::build_operator_state_http_response(
				format!(
					"GET {removed_path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
				)
				.as_bytes(),
			)
			.expect("removed route response should build"),
		)
		.expect("removed route response should be utf-8");

		assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
		assert!(response.ends_with("not found"));
	}
}
