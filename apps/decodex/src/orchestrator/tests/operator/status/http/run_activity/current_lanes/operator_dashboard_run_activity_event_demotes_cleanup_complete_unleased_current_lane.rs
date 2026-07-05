use crate::orchestrator::tests::operator::status::http::{
	self, ProjectRegistration, StateStore, TempDir, TestEnvVarGuard, Value, orchestrator,
};

#[test]
fn operator_dashboard_run_activity_event_demotes_cleanup_complete_unleased_current_lane() {
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
	let issue = http::sample_issue_with_sort_fields(
		"issue-xy-952",
		"XY-952",
		"Done",
		&[],
		Some(3),
		"2026-06-16T08:50:00Z",
	);
	let worktree_path = config.worktree_root().join("XY-952");

	http::git_status_success(
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

	http::seed_local_linear_execution_events(
		&state_store,
		&http::successful_linear_execution_history_comments_with_cleanup(&issue),
	);

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

	assert_eq!(payload["type"], "runActivity");
	assert_eq!(data["currentLanesComplete"], true);
	assert_eq!(data["currentLaneScope"], "complete");
	assert_eq!(data["currentLanes"].as_array().map(Vec::len), Some(0));
	assert_eq!(fingerprint["currentLanes"].as_array().map(Vec::len), Some(0));
	assert_eq!(data["presentation"]["current_lane_cards"].as_array().map(Vec::len), Some(0));
	assert_eq!(fingerprint["presentation"]["current_lane_cards"].as_array().map(Vec::len), Some(0));
	assert!(!data.to_string().contains("xy-952-attempt-2-1781598614"));
}
