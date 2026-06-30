use super::*;

use records::LinearExecutionEventRecord;

#[test]
fn failure_comments_use_repo_relative_worktree_paths() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	git_status_success(
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
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
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

#[test]
fn live_operator_status_classifies_missing_issue_ghost_lane_for_runtime_recovery() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("ghost current lane should be visible");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(run.run_id, "run-12");
	assert_eq!(run.issue_id, "PUB-012");
	assert_eq!(run.issue_identifier.as_deref(), Some("PUB-012"));
	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert!(run.lane_control_conditions.contains(&String::from("tracker_issue_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("worktree_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("control_channel_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("private_evidence_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("review_lineage_missing")));
	assert_eq!(project.attention_count, 1);
	assert!(!rendered.contains("Record the independent Decodex Review checkpoint"));
	assert!(!rendered.contains("review-handoff"));
}

#[test]
fn live_operator_status_classifies_invalid_local_issue_id_as_ghost_lane() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("ghost current lane should be visible");

	assert_eq!(run.issue_id, "PUB-012");
	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert!(run.lane_control_conditions.contains(&String::from("tracker_issue_missing")));
	assert!(
		!snapshot.warnings.iter().any(|warning| warning.contains("runtime_recovery_unavailable"))
	);
}

#[test]
fn live_operator_status_ignores_terminal_identifier_worktree_mapping_without_tracker_refresh() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let stale_issue_id = "PUB-001";
	let missing_worktree_path = config.worktree_root().join(stale_issue_id);

	state_store
		.record_run_attempt("run-01", stale_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			stale_issue_id,
			"x/pubfi-pub-001",
			&missing_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let history_lane = snapshot.history_lanes.first().expect("terminal history lane should render");

	assert!(snapshot.worktrees.is_empty());
	assert!(
		snapshot.warnings.contains(&String::from("stale_terminal_local_worktree_mapping_ignored"))
	);
	assert_eq!(history_lane.issue_id, stale_issue_id);
	assert_eq!(history_lane.ledger_outcome.ledger_status, "local_terminal_residue");
	assert_eq!(history_lane.ledger_outcome.final_outcome, "local_terminal_residue");
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.all(|issue_id| issue_id != stale_issue_id),
		"terminal local identifier id must not be sent to tracker refresh"
	);
	assert!(
		tracker.comment_queries.borrow().iter().all(|issue_id| issue_id != stale_issue_id),
		"terminal local identifier id must not be used for Linear ledger lookup"
	);
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("stale_terminal_local_worktree_mapping_ignored"));
	assert!(!rendered.contains("execution_ledger_status_unavailable"));
}

#[test]
fn live_operator_status_hydrates_terminal_identifier_history_with_review_checkpoint() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let protected_issue_id = "PUB-001";
	let issue = sample_issue_with_sort_fields(
		protected_issue_id,
		protected_issue_id,
		"In Review",
		&[],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![issue]);
	let missing_worktree_path = config.worktree_root().join(protected_issue_id);

	state_store
		.record_run_attempt("run-01", protected_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			protected_issue_id,
			"x/pubfi-pub-001",
			&missing_worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: protected_issue_id,
			run_id: "run-01",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let history_lane = snapshot.history_lanes.first().expect("terminal history lane should render");

	assert!(
		!snapshot.warnings.contains(&String::from("stale_terminal_local_worktree_mapping_ignored")),
		"review-authority mappings must not be classified as local residue"
	);
	assert!(
		snapshot.worktrees.iter().any(|worktree| worktree.issue_id == protected_issue_id),
		"review-authority worktree mapping must remain visible"
	);
	assert_ne!(history_lane.ledger_outcome.ledger_status, "local_terminal_residue");
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.any(|issue_id| issue_id == protected_issue_id),
		"review-authority terminal identifier id must still be sent to tracker refresh"
	);
	assert!(
		tracker.comment_queries.borrow().iter().any(|issue_id| issue_id == protected_issue_id),
		"review-authority terminal identifier id must still be used for Linear ledger lookup"
	);
}

#[test]
fn live_operator_status_hydrates_active_terminal_identifier_lane() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_issue_id = "PUB-001";
	let issue = sample_issue_with_sort_fields(
		active_issue_id,
		active_issue_id,
		"In Progress",
		&[crate::tracker::automation_active_label(config.service_id()).as_str()],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![issue]);
	let missing_worktree_path = config.worktree_root().join(active_issue_id);

	state_store
		.record_run_attempt("run-01", active_issue_id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal run attempt should record");
	state_store
		.upsert_lease("pubfi", active_issue_id, "run-01", "In Progress")
		.expect("active lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			active_issue_id,
			"x/pubfi-pub-001",
			&missing_worktree_path.display().to_string(),
		)
		.expect("active worktree mapping should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let history_lane = snapshot.history_lanes.first().expect("terminal history lane should render");

	assert_eq!(history_lane.issue_id, active_issue_id);
	assert_ne!(history_lane.ledger_outcome.ledger_status, "local_terminal_residue");
	assert!(
		!snapshot.warnings.contains(&String::from("stale_terminal_local_worktree_mapping_ignored")),
		"active lanes must not be classified as local residue"
	);
	assert!(
		tracker
			.refresh_queries
			.borrow()
			.iter()
			.flatten()
			.any(|issue_id| issue_id == active_issue_id),
		"active terminal identifier id must still be sent to tracker refresh"
	);
	assert!(
		tracker.comment_queries.borrow().iter().any(|issue_id| issue_id == active_issue_id),
		"active terminal identifier id must still be used for Linear ledger lookup"
	);
}

#[test]
fn live_operator_status_allows_ghost_recovery_when_worktree_mapping_path_is_missing() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_worktree_path = config.worktree_root().join("PUB-012");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&missing_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("ghost current lane should be visible");

	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(run.lane_control_conditions.contains(&String::from("worktree_mapping_path_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("worktree_missing")));
	assert!(!run.lane_control_conditions.contains(&String::from("retained_worktree_present")));
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_retained_worktree_exists() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	fs::create_dir_all(config.worktree_root().join("PUB-012"))
		.expect("retained worktree directory should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert_eq!(run.lane_control_next_action, "inspect_missing_issue_runtime_recovery_blockers");
	assert!(run.needs_attention);
	assert!(!run.counts_as_running);
	assert!(run.lane_control_conditions.contains(&String::from("retained_worktree_present")));
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_control_channel_row_exists() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let channel_path = temp_dir.path().join("missing-control-channel.json");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.publish_run_control_channel_for_active_attempt("run-12", 1, &channel_path, "local_file")
		.expect("control channel row should publish");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert_eq!(run.lane_control_next_action, "inspect_missing_issue_runtime_recovery_blockers");
	assert!(run.needs_attention);
	assert!(!run.counts_as_running);
	assert!(run.lane_control_conditions.contains(&String::from("control_channel_file_missing")));
	assert!(run.lane_control_conditions.contains(&String::from("control_channel_present")));
}

#[test]
fn ghost_lane_cleanup_status_blockers_treat_invalid_local_issue_id_as_missing_issue() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("invalid local issue id should be classified as a missing tracker issue");

	assert!(blockers.is_empty(), "missing issue with no live evidence should allow cleanup");
}

#[test]
fn ghost_lane_cleanup_status_blockers_preserve_live_blockers_after_invalid_issue_id_lookup() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let channel_path = temp_dir.path().join("missing-control-channel.json");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.publish_run_control_channel_for_active_attempt("run-12", 1, &channel_path, "local_file")
		.expect("control channel row should publish");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("invalid local issue id should still run local safety checks");

	assert!(blockers.contains(&String::from("control_channel_present")));
	assert!(blockers.contains(&String::from("control_channel_file_missing")));
}

#[test]
fn ghost_lane_cleanup_status_blockers_do_not_hide_validation_error_for_server_issue_id() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::with_refresh_error(
		Vec::new(),
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let issue_id = "00000000-0000-0000-0000-000000000012";

	state_store
		.record_run_attempt("run-12", issue_id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", issue_id, "run-12", "In Progress")
		.expect("lease should record");

	let error = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		issue_id,
		"run-12",
	)
	.expect_err("server issue id validation errors must remain tracker failures");

	assert!(error.to_string().contains("Argument Validation Error"));
}

#[test]
fn ghost_lane_cleanup_status_blockers_reject_live_process_evidence() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let worktree_path = config.worktree_root().join("PUB-012");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_activity_marker_for_process(&worktree_path, "run-12", 1, process::id())
		.expect("live process marker should write");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");

	assert!(blockers.contains(&String::from("process_alive")));
	assert!(blockers.contains(&String::from("retained_worktree_present")));
}

#[test]
fn ghost_lane_cleanup_status_blockers_reject_active_thread_evidence() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let worktree_path = config.worktree_root().join("PUB-012");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-12",
		1,
		Some("thread-12"),
		Some("turn-12"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("active thread marker should write");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");

	assert!(blockers.contains(&String::from("thread_active")));
	assert!(blockers.contains(&String::from("retained_worktree_present")));
}

#[test]
fn ghost_lane_cleanup_status_blockers_do_not_persist_marker_thread_identity() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let state_store = StateStore::open(&state_path).expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let worktree_path = config.worktree_root().join("PUB-012");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_thread_status_marker(
		&worktree_path,
		"run-12",
		1,
		Some("thread-12"),
		Some("turn-12"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("active thread marker should write");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");
	let connection = Connection::open(&state_path).expect("sqlite should open");
	let (thread_id, turn_id): (Option<String>, Option<String>) = connection
		.query_row(
			"SELECT thread_id, turn_id FROM run_attempts WHERE run_id = 'run-12'",
			[],
			|row| Ok((row.get(0)?, row.get(1)?)),
		)
		.expect("run attempt should exist");

	assert!(blockers.contains(&String::from("thread_active")));
	assert_eq!(thread_id, None);
	assert_eq!(turn_id, None);
}

#[test]
fn ghost_lane_cleanup_status_blockers_reject_existing_tracker_issue() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = sample_issue("In Progress", &[]);

	issue.id = String::from("PUB-012");
	issue.identifier = String::from("PUB-012");

	let tracker = FakeTracker::new(vec![issue]);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");

	assert!(blockers.contains(&String::from("tracker_issue_present")));
	assert!(blockers.contains(&String::from("issue_state:In Progress")));
}

#[test]
fn ghost_lane_cleanup_status_blockers_reject_recent_protocol_evidence() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let worktree_path = config.worktree_root().join("PUB-012");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-pub-012",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-12",
			attempt_number: 1,
			thread_id: None,
			turn_id: None,
			event_count: 1,
			last_event_type: "thread/status/changed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("recent protocol marker should write");

	rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");

	assert!(blockers.contains(&String::from("protocol_recent")));
	assert!(blockers.contains(&String::from("retained_worktree_present")));
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_review_lifecycle_exists() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let marker = ReviewHandoffMarker::new(
		"run-12",
		1,
		"x/pubfi-pub-012",
		"https://github.com/hack-ink/decodex/pull/12",
		"main",
		"x/pubfi-pub-012",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	);

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_review_handoff_marker(TEST_SERVICE_ID, "PUB-012", &marker)
		.expect("review lifecycle should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert!(run.lane_control_conditions.contains(&String::from("review_lifecycle_present")));
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_private_evidence_exists() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"diagnostic",
			serde_json::json!({"schema": "test.private/1"}),
		)
		.expect("private evidence should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert!(run.lane_control_conditions.contains(&String::from("private_evidence_present")));
}

#[test]
fn live_operator_status_allows_mcp_test_fixture_ghost_lane_cleanup_conditions() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_channel_path = config.worktree_root().join("missing-run-control.channel");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store.update_run_thread("run-12", "thread-12").expect("thread should record");
	state_store.update_run_turn("run-12", "turn-12").expect("turn should record");
	state_store
		.publish_run_control_channel_for_active_attempt(
			"run-12",
			1,
			&missing_channel_path,
			"local_file",
		)
		.expect("control channel row should publish");

	for (event_type, payload) in [
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "mcp-test",
				"action": "steer"
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "cli",
				"action": "interrupt",
				"requested": {
					"project_id": "pubfi",
					"issue_id": "PUB-012",
					"run_id": "run-12",
					"attempt_number": 1,
					"thread_id": "thread-12",
					"turn_id": "turn-12"
				}
			}),
		),
		(
			"lane_control/steer/requested",
			serde_json::json!({
				"source": "mcp-test",
				"method": "turn/steer"
			}),
		),
	] {
		state_store
			.append_private_execution_event("pubfi", "PUB-012", "run-12", 1, event_type, payload)
			.expect("mcp-test private evidence should record");
	}

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "ghost_lane");
	assert_eq!(run.policy_state, "runtime_recovery_required");
	assert_eq!(run.lane_control_next_action, "run_ghost_lane_recovery");
	assert!(
		run.lane_control_conditions
			.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
	);
	assert!(
		run.lane_control_conditions
			.contains(&String::from("mcp_test_fixture_protocol_or_thread_evidence_present"))
	);

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("cleanup status blockers should load");

	assert!(blockers.is_empty());
}

#[test]
fn live_operator_status_drops_cleanup_audited_mcp_test_fixture_ghost_lane() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_channel_path = config.worktree_root().join("missing-run-control.channel");
	let missing_worktree_path = config.worktree_root().join("PUB-012");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store.update_run_thread("run-12", "thread-12").expect("thread should record");
	state_store.update_run_turn("run-12", "turn-12").expect("turn should record");
	state_store
		.publish_run_control_channel_for_active_attempt(
			"run-12",
			1,
			&missing_channel_path,
			"local_file",
		)
		.expect("control channel row should publish");
	state_store
		.upsert_worktree(
			"pubfi",
			"PUB-012",
			"x/pubfi-012",
			&missing_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	append_mcp_test_fixture_control_private_events(&state_store);
	append_mcp_test_fixture_ghost_lane_cleanup_audit(&state_store);

	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		&tracker,
		&config,
		&workflow,
		&state_store,
		"PUB-012",
		"run-12",
	)
	.expect("prior cleanup audit should be accepted as safe recovery evidence");

	assert!(blockers.is_empty());

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(
		snapshot.current_lanes.is_empty(),
		"cleanup-audited fixture ghost lane must not remain current"
	);
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 0);
	assert_eq!(
		snapshot.recent_runs[0].ownership_state, "closed",
		"cleanup-audited fixture must not leave a projected leased recent run"
	);
	assert_eq!(
		snapshot.worktrees[0].ownership, "cleanup_only",
		"cleanup-audited fixture must not leave a current-lane worktree owner"
	);
}

#[test]
fn live_operator_status_keeps_cleanup_audited_mcp_fixture_blocked_when_worktree_exists() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let missing_channel_path = config.worktree_root().join("missing-run-control.channel");

	fs::create_dir_all(config.worktree_root().join("PUB-012"))
		.expect("retained worktree directory should exist");

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store.update_run_thread("run-12", "thread-12").expect("thread should record");
	state_store.update_run_turn("run-12", "turn-12").expect("turn should record");
	state_store
		.publish_run_control_channel_for_active_attempt(
			"run-12",
			1,
			&missing_channel_path,
			"local_file",
		)
		.expect("control channel row should publish");

	append_mcp_test_fixture_control_private_events(&state_store);
	append_mcp_test_fixture_ghost_lane_cleanup_audit(&state_store);

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert!(run.lane_control_conditions.contains(&String::from("retained_worktree_present")));
}

fn append_mcp_test_fixture_control_private_events(state_store: &StateStore) {
	for (event_type, payload) in [
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "mcp-test",
				"action": "steer"
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "cli",
				"action": "interrupt",
				"requested": {
					"project_id": "pubfi",
					"issue_id": "PUB-012",
					"run_id": "run-12",
					"attempt_number": 1,
					"thread_id": "thread-12",
					"turn_id": "turn-12"
				}
			}),
		),
		(
			"lane_control/steer/requested",
			serde_json::json!({
				"source": "mcp-test",
				"method": "turn/steer"
			}),
		),
	] {
		state_store
			.append_private_execution_event("pubfi", "PUB-012", "run-12", 1, event_type, payload)
			.expect("mcp-test private evidence should record");
	}
}

fn append_mcp_test_fixture_ghost_lane_cleanup_audit(state_store: &StateStore) {
	state_store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"ghost_lane_cleanup",
			serde_json::json!({
				"schema": "decodex.ghost_lane_recovery_private_event/1",
				"event": "ghost_lane_cleanup",
				"classification": "mcp_test_fixture_ghost_lane",
				"reason": "tracker_issue_missing_and_only_mcp_test_control_fixture_evidence",
				"issue_identifier": "PUBFI-012",
				"terminal_status": "terminal_guarded",
				"cleared_run_lease": true,
				"evidence": [
					"tracker_issue_missing",
					"worktree_mapping_path_missing",
					"worktree_missing",
					"control_channel_file_missing",
					"mcp_test_fixture_control_channel_row_present",
					"mcp_test_fixture_private_control_evidence_present",
					"review_lineage_missing"
				],
				"blockers": [],
				"next_action": "ordinary automation may continue after status readback confirms no current attention lane"
			}),
		)
		.expect("cleanup audit should record");
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_review_checkpoint_exists() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: "PUB-012",
			run_id: "run-12",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert!(
		run.lane_control_conditions.contains(&String::from("review_policy_checkpoint_present"))
	);
}

#[test]
fn live_operator_status_blocks_missing_issue_ghost_cleanup_when_pr_lineage_exists() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-012",
			issue_identifier: "PUB-012",
			run_id: "run-12",
			attempt_number: 1,
		},
		"closeout",
		String::from("2026-06-18T00:00:00Z"),
		"closeout",
	);

	event.branch = Some(String::from("x/pubfi-pub-012"));
	event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/12"));
	event.pr_head_sha = Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"));
	event.pr_base_ref = Some(String::from("main"));
	event.commit_sha = Some(String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d7"));
	event.summary = Some(String::from("Recorded retained closeout."));

	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("lease should record");
	state_store.record_linear_execution_event(&event).expect("linear event should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should be visible");

	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.policy_state, "runtime_recovery_blocked");
	assert!(run.lane_control_conditions.contains(&String::from("pr_or_review_lineage_present")));
}
