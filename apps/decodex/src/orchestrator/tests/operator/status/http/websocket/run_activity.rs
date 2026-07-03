use crate::orchestrator::tests::operator::status::http::{
	self, Arc, CodexAccountActivitySummary, CodexAccountMarker, DashboardEventHub, Mutex,
	OperatorControlRequests, ProjectRegistration, ProtocolActivityMarker, ProtocolActivitySummary,
	PublishedOperatorSnapshot, StateStore, TcpListener, TempDir, TestEnvVarGuard, Value,
	Write as _, orchestrator, slice, state, thread,
};
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
	let (mut client, response, mut frame) = super::open_dashboard_websocket_client(address);

	assert!(response.starts_with("HTTP/1.1 101 Switching Protocols\r\n"));

	client
		.write_all(&super::websocket_client_text_frame(
			r#"{"type":"subscribe","requestId":"sub-filter","projectId":"pubfi","runId":"run-2"}"#,
		))
		.expect("client should send subscription");

	let _subscribe_ack = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
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

	let activity = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
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
	let (mut client, response, mut frame) = super::open_dashboard_websocket_client(address);

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
			.write_all(&super::websocket_client_text_frame(&control_message))
			.expect("client should send unsupported dashboard control");

		let ack = super::read_websocket_json_until(&mut client, &mut frame, |payload| {
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
		super::websocket_text_payload(&message).expect("event should be a text frame");
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
