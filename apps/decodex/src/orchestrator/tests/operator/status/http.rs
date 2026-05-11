use std::net::SocketAddr;

#[test]
fn operator_state_endpoint_serves_snapshot_json() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
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
			event_count: 4,
			last_event_type: "item/tool/call/response",
			child_agent_activity: Some(&ChildAgentActivitySummary {
				buckets: vec![state::ChildAgentActivityBucket {
					name: String::from("Browser/Image"),
					wall_seconds: 41,
					event_count: 4,
					tool_call_count: 2,
					input_tokens: 0,
					output_tokens: 0,
					output_bytes: 240_000,
				}],
				current_bucket: Some(String::from("Model")),
				current_detail: Some(String::from("waiting after tool output")),
				current_started_unix_epoch: None,
				current_elapsed_seconds: Some(0),
				wall_seconds: 693,
				event_count: 4,
				tool_call_count: 2,
				input_tokens_current: Some(135_000),
				input_tokens_max: Some(135_000),
				input_tokens_cumulative: 6_510_000,
				output_tokens_cumulative: 18_000,
				largest_tool_output_bytes: Some(240_000),
				largest_tool_output_tool: Some(String::from("view_image")),
					large_output_warnings: vec![String::from(
						"view_image repeated 2 large outputs; largest 240000 bytes",
					)],
				}),
				protocol_activity: None,
			},
		)
	.expect("child activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_vec(&snapshot).expect("snapshot json should serialize");
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_STATE_ENDPOINT_PATH
			)
			.as_bytes(),
			Some(snapshot_json.as_slice()),
			OperatorSnapshotReadiness::Ready,
		)
		.expect("response build should succeed"),
	)
	.expect("response should be utf-8");
	let (status_line, body) =
		response.split_once("\r\n").expect("response should contain a status line");
	let body = body.split_once("\r\n\r\n").expect("response should contain a body").1;
	let served_snapshot: Value = serde_json::from_str(body).expect("body should be valid json");

	assert_eq!(status_line, "HTTP/1.1 200 OK");
	assert_eq!(served_snapshot["project_id"], "pubfi");
	assert_eq!(served_snapshot["run_limit"], 10);
	assert_eq!(served_snapshot["active_runs"][0]["run_id"], "run-1");
	assert_eq!(served_snapshot["active_runs"][0]["status"], "running");
	assert_eq!(served_snapshot["active_runs"][0]["attempt_status"], "running");
	assert_eq!(served_snapshot["active_runs"][0]["phase"], "executing");
	assert_eq!(served_snapshot["active_runs"][0]["queue_lease_state"], "held");
	assert_eq!(served_snapshot["active_runs"][0]["execution_liveness"], "process_alive");
	assert_eq!(
		served_snapshot["active_runs"][0]["child_agent_activity"]["buckets"][0]["name"],
		"Browser/Image"
	);
	assert_eq!(
		served_snapshot["active_runs"][0]["child_agent_activity"]["input_tokens_max"],
		135_000
	);
	assert_eq!(served_snapshot["queued_candidates"], Value::Array(Vec::new()));
	assert_eq!(served_snapshot["worktrees"][0]["worktree_path"], ".worktrees/PUB-101");
}

#[test]
fn operator_state_endpoint_serializes_closed_queue_classification() {
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("pubfi"),
		run_limit: 10,
		warnings: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		accounts: Vec::new(),
		active_runs: vec![],
		recent_runs: vec![],
		history_lanes: vec![],
		queued_candidates: vec![orchestrator::OperatorQueuedIssueStatus {
			issue_id: String::from("issue-closed"),
			issue_identifier: String::from("PUB-104"),
			title: String::from("Retire closed queue residue"),
			state: String::from("Done"),
			priority: Some(1),
			created_at: String::from("2026-03-14T09:58:00Z"),
			classification: String::from("closed"),
			reason: String::from("terminal_state"),
			attention: None,
			blocker_identifiers: vec![],
		}],
		worktrees: vec![],
		post_review_lanes: vec![],
	};
	let snapshot_json = serde_json::to_vec(&snapshot).expect("snapshot json should serialize");
	let response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_STATE_ENDPOINT_PATH
			)
			.as_bytes(),
			Some(snapshot_json.as_slice()),
			OperatorSnapshotReadiness::Ready,
		)
		.expect("response build should succeed"),
	)
	.expect("response should be utf-8");
	let body = response.split_once("\r\n\r\n").expect("response should contain a body").1;
	let served_snapshot: Value = serde_json::from_str(body).expect("body should be valid json");

	assert_eq!(served_snapshot["queued_candidates"][0]["classification"], "closed");
	assert_eq!(served_snapshot["queued_candidates"][0]["reason"], "terminal_state");
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
				None,
				OperatorSnapshotReadiness::SnapshotUnavailable,
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
		assert!(response.contains("State fetch"));
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
		assert!(response.contains("Running Lanes"));
		assert!(response.contains("Intake Queue"));
		assert!(response.contains("At capacity"));
		assert!(response.contains("Review &amp; Landing"));
		assert!(response.contains("Run History"));
		assert!(response.contains("historyLedgerOutcome"));
		assert!(response.contains("Run history unavailable"));
		assert!(response.contains("renderHistoryLedgerFacts"));
		assert!(response.contains("Recovery Worktrees"));
		assert!(response.contains("Lane activity"));
		assert!(response.contains("agent idle"));
		assert!(response.contains("Child agent"));
		assert!(response.contains("Agent now"));
		assert!(response.contains("Current window"));
		assert!(response.contains("Peak window"));
		assert!(!response.contains("same as current"));
		assert!(response.contains("Cumulative input"));
		assert!(response.contains("Current context window from the latest child-agent event."));
		assert!(response.contains("Total input tokens processed across child-agent events."));
		assert!(response.contains("child_agent_activity"));
		assert!(response.contains("renderChildAgentBreakdown"));
		assert!(response.contains("Debug details"));
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
		"/state",
		"/readyz",
		"/dashboard/control",
		"WebSocket",
		"applyDashboardRunActivity",
		"sendDashboardControl",
		"data-dashboard-control=\"interruptRun\"",
		"aria-label=\"Stop this active Decodex work\"",
		"if (!interruptEnabled) {",
		"controlAck",
	] {
		assert!(response.contains(required), "missing required dashboard control marker `{required}`");
	}
	for forbidden in [
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
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
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
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
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
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
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
			&server_state_store,
			Duration::from_secs(30),
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

	let deadline = Instant::now() + Duration::from_secs(1);
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
fn operator_dashboard_websocket_accepts_subscription_and_project_pause_control() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
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
			&server_state_store,
			Duration::from_secs(30),
		)
		.expect("websocket handler should complete after client disconnect");
	});
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

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

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"pause-1","action":"pauseProject","projectId":"pubfi"}"#,
		))
		.expect("client should send pause");

	let pause_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "pause-1"
	});

	assert_eq!(pause_ack["payload"]["accepted"], true);
	assert_eq!(pause_ack["payload"]["status"], "paused");
	assert!(
		!state_store
			.list_projects()
			.expect("projects should list")
			.into_iter()
			.find(|project| project.service_id() == "pubfi")
			.expect("project should remain registered")
			.enabled()
	);

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"resume-1","action":"resumeProject","projectId":"pubfi"}"#,
		))
		.expect("client should send resume");

	let resume_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "resume-1"
	});

	assert_eq!(resume_ack["payload"]["accepted"], true);
	assert_eq!(resume_ack["payload"]["status"], "resumed");
	assert!(
		state_store
			.list_projects()
			.expect("projects should list")
			.into_iter()
			.find(|project| project.service_id() == "pubfi")
			.expect("project should remain registered")
			.enabled()
	);

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
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
			&server_state_store,
			Duration::from_secs(30),
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
			&server_state_store,
			Duration::from_secs(30),
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
			"activeRuns": [
				{ "project_id": "pubfi", "issue_id": "PUB-101", "run_id": "run-1" },
				{ "project_id": "pubfi", "issue_id": "PUB-102", "run_id": "run-2" },
				{ "project_id": "rsnap", "issue_id": "RS-1", "run_id": "run-2" }
			]
		}),
	);

	let activity = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "runActivity"
	});
	let active_runs = activity["payload"]["activeRuns"]
		.as_array()
		.expect("active runs should list");

	assert_eq!(active_runs.len(), 1);
	assert_eq!(active_runs[0]["project_id"], "pubfi");
	assert_eq!(active_runs[0]["issue_id"], "PUB-102");
	assert_eq!(active_runs[0]["run_id"], "run-2");

	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_dashboard_websocket_interrupt_control_stops_active_run_process() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot::default()));
	let state_store = Arc::new(StateStore::open_in_memory().expect("state store should open"));
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let dashboard_events = DashboardEventHub::default();
	let server_snapshot = Arc::clone(&snapshot);
	let server_state_store = Arc::clone(&state_store);
	let server_dashboard_events = dashboard_events.clone();
	let interrupter_calls = dashboard_run_interrupter_calls_for_test();

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
	state::write_run_operation_marker_for_process(
		&worktree_path,
		"run-1",
		1,
		4_242,
		RUN_OPERATION_AGENT_RUN,
	)
	.expect("run operation marker should write");
	interrupter_calls
		.lock()
		.expect("dashboard run interrupter calls should not be poisoned")
		.clear();

	let _interrupter_guard =
		orchestrator::install_dashboard_run_interrupter_for_test(fake_dashboard_run_interrupter);
	let server = thread::spawn(move || {
		let (stream, _) = listener.accept().expect("listener should accept a connection");

		orchestrator::handle_operator_state_endpoint_connection(
			stream,
			&server_snapshot,
			&server_dashboard_events,
			&server_state_store,
			Duration::from_secs(30),
		)
		.expect("websocket handler should complete after client disconnect");
	});
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	let interrupt_message = format!(
		r#"{{"type":"control","requestId":"interrupt-1","action":"interruptRun","projectId":"{}","issueId":"{}","runId":"run-1"}}"#,
		config.service_id(),
		issue.id,
	);
	client
		.write_all(&websocket_client_text_frame(&interrupt_message))
		.expect("client should send interrupt");

	let interrupt_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "interrupt-1"
	});

	assert_eq!(interrupt_ack["payload"]["accepted"], true);
	assert_eq!(interrupt_ack["payload"]["status"], "interrupted");
	assert_eq!(interrupt_ack["payload"]["projectId"], config.service_id());
	assert_eq!(interrupt_ack["payload"]["issueId"], issue.id);
	assert_eq!(interrupt_ack["payload"]["runId"], "run-1");
	assert!(
		interrupt_ack["payload"]["message"]
			.as_str()
			.expect("interrupt ack message should be text")
			.contains("process 4242")
	);

	let calls = interrupter_calls
		.lock()
		.expect("dashboard run interrupter calls should not be poisoned");

	assert_eq!(calls.len(), 1);
	assert_eq!(calls[0], 4_242);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run lookup should succeed")
			.expect("run should remain recorded")
			.status(),
		"interrupted",
	);
	assert!(
		state_store
			.lease_for_issue(&issue.id)
			.expect("lease lookup should succeed")
			.is_none(),
		"interrupt should release the queue lease"
	);

	drop(calls);
	drop(client);

	dashboard_events.close_clients_for_test();
	server.join().expect("server thread should complete");
}

#[test]
fn operator_dashboard_websocket_interrupt_control_reports_validation_errors() {
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
			&server_state_store,
			Duration::from_secs(30),
		)
		.expect("websocket handler should complete after client disconnect");
	});
	let (mut client, response, mut frame) = open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"missing-project","action":"pauseProject"}"#,
		))
		.expect("client should send missing-project control");

	let missing_project_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "missing-project"
	});

	assert_eq!(missing_project_ack["payload"]["accepted"], false);
	assert_eq!(missing_project_ack["payload"]["status"], "missing_project");

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"missing-run","action":"interruptRun","projectId":"pubfi","issueId":"PUB-101"}"#,
		))
		.expect("client should send missing-run control");

	let missing_run_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "missing-run"
	});

	assert_eq!(missing_run_ack["payload"]["accepted"], false);
	assert_eq!(missing_run_ack["payload"]["status"], "missing_run");

	client
		.write_all(&websocket_client_text_frame(
			r#"{"type":"control","requestId":"unknown-run","action":"interruptRun","projectId":"pubfi","issueId":"PUB-101","runId":"missing"}"#,
		))
		.expect("client should send unknown-run control");

	let unknown_run_ack = read_websocket_json_until(&mut client, &mut frame, |payload| {
		payload["type"] == "controlAck" && payload["payload"]["requestId"] == "unknown-run"
	});

	assert_eq!(unknown_run_ack["payload"]["accepted"], false);
	assert_eq!(unknown_run_ack["payload"]["status"], "failed");
	assert!(
		unknown_run_ack["payload"]["message"]
			.as_str()
			.expect("interrupt ack message should be text")
			.contains("not recorded")
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
		.set_read_timeout(Some(Duration::from_secs(1)))
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

fn dashboard_run_interrupter_calls_for_test() -> &'static Mutex<Vec<u32>> {
	static CALLS: std::sync::OnceLock<Mutex<Vec<u32>>> = std::sync::OnceLock::new();

	CALLS.get_or_init(|| Mutex::new(Vec::new()))
}

fn fake_dashboard_run_interrupter(process_id: u32) -> Result<()> {
	dashboard_run_interrupter_calls_for_test()
		.lock()
		.expect("dashboard run interrupter calls should not be poisoned")
		.push(process_id);

	Ok(())
}

fn read_websocket_json_until(
	client: &mut TcpStream,
	frame: &mut Vec<u8>,
	matches: impl Fn(&Value) -> bool,
) -> Value {
	let deadline = Instant::now() + Duration::from_secs(1);
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
fn operator_dashboard_run_activity_event_summarizes_active_runs() {
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

	assert_eq!(payload["type"], "runActivity");
	assert_eq!(data["activeRuns"][0]["run_id"], "run-1");
	assert_eq!(data["activeRuns"][0]["project_id"], "pubfi");
	assert_eq!(data["activeRuns"][0]["protocol_activity"]["waiting_reason"], "model");
	assert_eq!(data["activeRuns"][0]["account"]["account_fingerprint"], "acct-1");
	assert_eq!(data["activeRuns"][0]["accounts"][0]["account_fingerprint"], "acct-1");
}

#[test]
fn operator_state_endpoint_reads_complete_headers_before_parsing() {
	const SNAPSHOT_UNIX_EPOCH: i64 = 1_774_000_000;

	let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
	let address = listener.local_addr().expect("listener address should resolve");
	let snapshot = Arc::new(Mutex::new(PublishedOperatorSnapshot {
		snapshot_json: Some(br#"{"status":"ok"}"#.to_vec()),
		last_publish_unix_epoch: Some(SNAPSHOT_UNIX_EPOCH),
	}));
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
			&server_state_store,
			Duration::from_secs(30),
		)
		.expect("handler should accept segmented headers");
	});
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut response = String::new();

	client.write_all(b"GET /st").expect("client should write first request fragment");

	thread::sleep(Duration::from_millis(10));

	client
		.write_all(b"ate HTTP/1.1\r\nHost: localhost\r\n\r\n")
		.expect("client should write second request fragment");
	client.shutdown(Shutdown::Write).expect("client should close the request body stream");
	client.read_to_string(&mut response).expect("client should read response");
	server.join().expect("server thread should complete");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(response.contains(&format!(
		"X-Decodex-Snapshot-Unix-Epoch: {SNAPSHOT_UNIX_EPOCH}\r\n"
	)));
	assert!(response.ends_with("{\"status\":\"ok\"}"));
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
			&server_state_store,
			Duration::from_secs(30),
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
fn operator_state_endpoint_serves_liveness_and_readiness_probes() {
	let live_response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_LIVE_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("live response should build"),
	)
	.expect("live response should be utf-8");

	assert!(live_response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(live_response.ends_with("ok"));

	let ready_unavailable = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_READY_ENDPOINT_PATH
			)
			.as_bytes(),
			None,
			OperatorSnapshotReadiness::SnapshotUnavailable,
		)
		.expect("ready response should build"),
	)
	.expect("ready response should be utf-8");

	assert!(ready_unavailable.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
	assert!(ready_unavailable.ends_with("snapshot_unavailable"));

	let ready_response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_READY_ENDPOINT_PATH
			)
			.as_bytes(),
			Some(br#"{"status":"ok"}"#),
			OperatorSnapshotReadiness::Ready,
		)
		.expect("ready response should build"),
	)
	.expect("ready response should be utf-8");

	assert!(ready_response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert!(ready_response.ends_with("ready"));
}

#[test]
fn operator_state_endpoint_reports_stale_snapshots_as_not_ready() {
	let stale_response = String::from_utf8(
		orchestrator::build_operator_state_http_response(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
				orchestrator::OPERATOR_READY_ENDPOINT_PATH
			)
			.as_bytes(),
			Some(br#"{"status":"ok"}"#),
			OperatorSnapshotReadiness::SnapshotStale,
		)
		.expect("stale ready response should build"),
	)
	.expect("stale ready response should be utf-8");

	assert!(stale_response.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
	assert!(stale_response.ends_with("snapshot_stale"));
}

#[test]
fn operator_snapshot_readiness_handles_timestamp_edges() {
	for (last_publish, now, threshold, expected) in [
		(
			Some(200),
			100,
			Duration::from_secs(30),
			OperatorSnapshotReadiness::SnapshotStale,
		),
		(
			Some(100),
			101,
			Duration::from_secs(u64::MAX),
			OperatorSnapshotReadiness::Ready,
		),
	] {
		assert_eq!(
			orchestrator::operator_snapshot_readiness(last_publish, now, threshold),
			expected
		);
	}
}
