use crate::orchestrator::tests::operator::status::http::{
	self, CodexAccountActivitySummary, CodexAccountMarker, ProjectRegistration,
	ProtocolActivityMarker, ProtocolActivitySummary, StateStore, TempDir, TestEnvVarGuard, Value,
	orchestrator, slice, state,
};

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
		http::websocket_text_payload(&message).expect("event should be a text frame");
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
