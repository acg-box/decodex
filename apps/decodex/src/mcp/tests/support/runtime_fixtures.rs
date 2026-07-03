use std::{fs, path::Path, process};

use serde_json::Value;

use crate::{
	mcp::tests::support::{repo_fixtures, stdio},
	runtime,
	state::{self, ProtocolActivityEventSummary, ProtocolActivitySummary, StateStore},
};

pub(in crate::mcp::tests) fn seed_project_runtime_for_mcp_resources(
	repo_root: &Path,
	config_path: &Path,
) {
	let state_store = runtime::open_runtime_store().expect("runtime store should open");

	repo_fixtures::write_project_config(config_path, repo_root);
	repo_fixtures::write_project_workflow(repo_root);
	runtime::register_project_config(&state_store, config_path, true)
		.expect("project should register");

	for index in 1..=12 {
		let issue_id = format!("PUB-{index:03}");
		let run_id = format!("run-{index:02}");
		let worktree_path = repo_root.join(format!("worktrees/{issue_id}"));
		let attempt_status = if index == 12 { "running" } else { "succeeded" };

		state_store
			.upsert_worktree(
				"pubfi",
				&issue_id,
				&format!("x/pubfi-{index:03}"),
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");
		state_store
			.record_run_attempt(&run_id, &issue_id, 1, attempt_status)
			.expect("run attempt should record");
		state_store
			.append_event(&run_id, 1, "turn/completed", r#"{"status":"completed"}"#)
			.expect("event should record");

		if index == 12 {
			seed_mcp_lane_runtime_markers(&state_store, &worktree_path, &run_id);
			seed_mcp_lane_runtime_activity(&state_store, &run_id);
		}
	}
}

pub(in crate::mcp::tests) fn seed_mcp_test_private_control_evidence() {
	let state_store = runtime::open_runtime_store().expect("runtime store should open");

	state_store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "mcp-test",
				"action": "project_control_fixture"
			}),
		)
		.expect("mcp-test private evidence should record");
}

pub(in crate::mcp::tests) fn seed_mcp_lane_runtime_activity(
	state_store: &StateStore,
	run_id: &str,
) {
	state_store
		.append_event(
			run_id,
			2,
			"configWarning",
			r#"{"summary":"config at /private/worktree using GITHUB_PAT_Y"}"#,
		)
		.expect("warning event should record");
	state_store
		.append_event(
			run_id,
			3,
			"error",
			r#"{"error":{"codexErrorInfo":"failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK"}}"#,
		)
		.expect("error event should record");
	state_store
		.append_event(
			run_id,
			4,
			"configWarning",
			r#"{"summary":"state marker under /srv/decodex/runtime"}"#,
		)
		.expect("generic path warning event should record");
	state_store
				.append_event(
					run_id,
					5,
					"error",
					r#"{"error":{"codexErrorInfo":"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456"}}"#,
				)
				.expect("token-shaped error event should record");
	state_store
		.append_event(
			run_id,
			6,
			"error",
			r#"{"error":{"codexErrorInfo":"upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U"}}"#,
		)
		.expect("bare token-shaped error event should record");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: vec![
			ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("config at /private/worktree using GITHUB_PAT_Y")),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK",
				)),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker under /srv/decodex/runtime")),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456",
				)),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U",
				)),
			},
		],
	};

	state_store
		.record_run_activity_summary(run_id, 1, None, Some(&protocol_activity))
		.expect("activity summary should record");
}

pub(in crate::mcp::tests) fn seed_mcp_lane_runtime_markers(
	state_store: &StateStore,
	worktree_path: &Path,
	run_id: &str,
) {
	fs::create_dir_all(worktree_path).expect("worktree path should exist");

	let control_dir = worktree_path.join(".decodex-run-control");
	let channel_path = control_dir.join("run-12-1.channel");

	fs::create_dir_all(&control_dir).expect("run-control channel dir should exist");
	fs::write(&channel_path, "ready\n").expect("run-control channel should write");

	state_store
		.upsert_lease("pubfi", "PUB-012", run_id, "In Progress")
		.expect("lease should record");
	state_store.update_run_thread(run_id, "thread-12").expect("thread should record");
	state_store.update_run_turn(run_id, "turn-12").expect("turn should record");
	state_store
		.publish_run_control_channel_for_active_attempt(run_id, 1, &channel_path, "local_file")
		.expect("control channel should publish")
		.expect("active control channel should exist");

	state::write_run_activity_marker_for_process(worktree_path, run_id, 1, process::id())
		.expect("activity marker should record process");
	state::write_run_thread_marker(worktree_path, run_id, 1, "thread-12")
		.expect("thread marker should record");
	state::write_run_turn_marker(worktree_path, run_id, 1, "turn-12")
		.expect("turn marker should record");
}

#[test]
fn mcp_project_fixture_runtime_store_stays_under_isolated_home() {
	let operator_runtime_db =
		runtime::runtime_db_path().expect("operator runtime path should resolve");
	let repo = repo_fixtures::test_repo();
	let _runtime_home_guard = repo_fixtures::isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	seed_project_runtime_for_mcp_resources(repo.path(), &config_path);

	stdio::run_stdio_with_context(
		stdio::project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"steer","projectId":"pubfi","issue":"PUB-012","runId":"run-12","expectedTurnId":"turn-12","message":"Please stop after the current safe point.","authority":{"reason":"operator requested steer","source":"mcp-test","inspectedRunId":"run-12","expectedTurnId":"turn-12"}}}}"#,
	);

	let fixture_runtime_db =
		runtime::runtime_db_path().expect("fixture runtime path should resolve");
	let state_store = runtime::open_runtime_store().expect("fixture runtime store should open");
	let events = state_store
		.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
		.expect("fixture private evidence should read");

	assert_ne!(fixture_runtime_db, operator_runtime_db);
	assert!(fixture_runtime_db.starts_with(repo.path()));
	assert!(!events.is_empty());
	assert!(
		events
			.iter()
			.all(|event| event.payload().get("source").and_then(Value::as_str) == Some("mcp-test")),
		"mcp fixture private evidence should stay in isolated runtime store"
	);
}
