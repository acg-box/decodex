use crate::orchestrator::tests::operator::status::http::{
	self, ProjectRegistration, StateStore, TempDir, TestEnvVarGuard, Value, orchestrator, state,
};

#[test]
fn operator_dashboard_run_activity_event_keeps_unleased_app_server_current_lane() {
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

	http::git_status_success(
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
	http::rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let event =
		orchestrator::build_operator_run_activity_event(&state_store).expect("event should build");
	let message =
		orchestrator::dashboard_websocket_message(event.event.event_type, &event.event.payload)
			.expect("event should serialize");
	let (payload, _consumed) =
		http::websocket_text_payload(&message).expect("event should be a text frame");
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
	assert_eq!(data["presentation"]["current_lane_cards"].as_array().map(Vec::len), Some(1));
	assert_eq!(data["presentation"]["current_lane_cards"][0]["run_id"], "run-1");
	assert_eq!(data["presentation"]["current_lane_cards"][0]["run"]["thread_status"], "active");
}
