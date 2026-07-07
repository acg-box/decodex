use crate::orchestrator::tests::operator::status::running_lanes::{
	self, ProjectRegistration, ReviewLifecycleHandoffFixture, StateStore, fs, orchestrator, state,
};

#[test]
fn operator_status_projects_terminal_finalized_repair_stale_lifecycle_authority() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let registration = ProjectRegistration::from_config(
		config.service_id(),
		&running_lanes::service_config_path(config.repo_root()),
		&config,
		true,
		"test-fingerprint",
	);
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let old_head_oid = "1111111111111111111111111111111111111111";
	let repaired_head_oid = "2222222222222222222222222222222222222222";

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.upsert_review_lifecycle_handoff_fixture(
			"pubfi",
			&issue.id,
			&ReviewLifecycleHandoffFixture::new(
				"run-old",
				1,
				"x/pubfi-pub-101",
				"https://github.com/hack-ink/decodex/pull/101",
				"main",
				"x/pubfi-pub-101",
				old_head_oid,
			),
		)
		.expect("old review lifecycle should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"review_completion_intent",
			serde_json::json!({
				"path": "review_repair",
				"mode": "repair",
				"branch": "x/pubfi-pub-101",
				"worktree_path": worktree_path.display().to_string(),
				"pr_url": "https://github.com/hack-ink/decodex/pull/101",
				"pr_base_ref": "main",
				"pr_head_ref": "x/pubfi-pub-101",
				"pr_head_oid": repaired_head_oid,
				"summary": "Review repair is clean."
			}),
		)
		.expect("repair intent should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"terminal_finalize",
			serde_json::json!({
				"path": "review_repair",
				"mode": "repair",
				"branch": "x/pubfi-pub-101",
				"worktree_path": worktree_path.display().to_string(),
			}),
		)
		.expect("terminal finalize event should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("terminal-pending run should remain inspectable in recent runs");

	assert_eq!(run.status, "review_repair_pending");
	assert_eq!(run.phase, "terminal_pending");
	assert_eq!(
		run.wait_reason.as_deref(),
		Some("review_repair_writeback_stale_lifecycle_authority")
	);
	assert_eq!(run.current_operation, state::RUN_OPERATION_REVIEW_WRITEBACK);
}
