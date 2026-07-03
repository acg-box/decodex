use crate::orchestrator::tests::operator::status::http::{
	self, ProjectRegistration, StateStore, TempDir, TestEnvVarGuard, Value, orchestrator,
};

#[test]
fn operator_dashboard_run_activity_event_includes_disabled_project_current_lanes() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("temp path should be UTF-8"));
	let (_temp_dir, config, _workflow) = http::temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let observer_store = StateStore::open(&state_path).expect("observer store should open");
	let writer_store = StateStore::open(&state_path).expect("writer store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&http::service_config_path(config.repo_root()),
		&config,
		false,
		"test-fingerprint",
	);
	let issue = http::sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	http::git_status_success(
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
	let message =
		orchestrator::dashboard_websocket_message(event.event.event_type, &event.event.payload)
			.expect("event should serialize");
	let (payload, _consumed) =
		http::websocket_text_payload(&message).expect("event should be a text frame");
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
	assert_eq!(data["presentation"]["current_lane_cards"].as_array().map(Vec::len), Some(1));
	assert_eq!(data["presentation"]["current_lane_cards"][0]["run_id"], "run-disabled-active");
}
