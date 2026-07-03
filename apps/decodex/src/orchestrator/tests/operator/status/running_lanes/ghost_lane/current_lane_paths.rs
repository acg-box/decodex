use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, WorktreeSpec, orchestrator, state,
};

#[test]
fn failure_comments_use_repo_relative_worktree_paths() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: String::from("PUB-101"),
		path: config.repo_root().join(".worktrees/PUB-101"),
		reused_existing: true,
	};

	assert_eq!(orchestrator::relative_worktree_path(&config, &worktree), ".worktrees/PUB-101");
}

#[test]
fn operator_status_snapshot_includes_current_lanes_and_repo_relative_paths() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	running_lanes::git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.update_run_thread("run-1", "thread-1").expect("thread id should attach");
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
	state_store
		.append_event("run-1", 1, "turn/completed", "{\"turn\":\"1\"}")
		.expect("event should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert_eq!(snapshot.project_id, "pubfi");
	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(snapshot.current_lanes[0].project_id, "pubfi");
	assert_eq!(snapshot.current_lanes[0].project_display_name, "hack-ink/pubfi-mono-v2");
	assert_eq!(snapshot.current_lanes[0].run_id, "run-1");
	assert_eq!(snapshot.current_lanes[0].phase, "executing");
	assert_eq!(snapshot.current_lanes[0].current_operation, state::RUN_OPERATION_AGENT_RUN);
	assert_eq!(snapshot.current_lanes[0].policy_state, "allowed");
	assert_eq!(snapshot.current_lanes[0].lane_control_next_action, "continue_owned_attempt");
	assert!(
		snapshot.current_lanes[0]
			.loop_status
			.as_ref()
			.and_then(|status| status.review.as_ref())
			.is_none(),
		"ordinary running lanes must not synthesize a pending review checkpoint"
	);
	assert_eq!(snapshot.current_lanes[0].thread_id.as_deref(), Some("thread-1"));
	assert_eq!(snapshot.current_lanes[0].branch_name.as_deref(), Some("x/pubfi-pub-101"));
	assert_eq!(snapshot.current_lanes[0].worktree_path.as_deref(), Some(".worktrees/PUB-101"));
	assert!(snapshot.current_lanes[0].last_run_activity_at.is_some());
	assert!(snapshot.current_lanes[0].last_progress_at.is_some());
	assert!(!snapshot.current_lanes[0].suspected_stall);
	assert_eq!(snapshot.current_lanes[0].last_event_type.as_deref(), Some("turn/completed"));
	assert_eq!(snapshot.worktrees[0].worktree_path, ".worktrees/PUB-101");
	assert_eq!(snapshot.worktrees[0].ownership, "current_lane");
	assert!(snapshot.worktrees[0].ownership_reason.contains("Current lane `run-1`"));

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.current_lane_count, 1);
	assert_eq!(
		project.retained_worktree_count, 0,
		"current lane worktrees must not inflate project recovery counts"
	);
	assert_eq!(project.connector_state, "ok");
	assert!(project.last_activity_at.is_some());

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(!rendered.contains("Record the independent Decodex Review checkpoint"));
}

#[test]
fn operator_status_does_not_synthesize_review_for_continuation_pending_attempt() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "continuation_pending")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.status, "continuation_pending");
	assert_eq!(run.phase, "waiting_continuation");
	assert_eq!(run.current_operation, state::RUN_OPERATION_WAITING_EXTERNAL);
	assert_eq!(run.policy_state, "allowed");
	assert!(loop_status.review.is_none());
	assert_eq!(snapshot.projects[0].waiting_lane_count, 1);
	assert!(!rendered.contains("Record the independent Decodex Review checkpoint"));
}
