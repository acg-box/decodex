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
	assert!(!snapshot.warnings.iter().any(|warning| warning.contains("runtime_recovery_unavailable")));
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
	assert_eq!(
		run.lane_control_next_action,
		"inspect_missing_issue_runtime_recovery_blockers"
	);
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
		.publish_run_control_channel_for_active_attempt(
			"run-12",
			1,
			&channel_path,
			"local_file",
		)
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
	assert_eq!(
		run.lane_control_next_action,
		"inspect_missing_issue_runtime_recovery_blockers"
	);
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
		.publish_run_control_channel_for_active_attempt(
			"run-12",
			1,
			&channel_path,
			"local_file",
		)
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
		run.lane_control_conditions
			.contains(&String::from("review_policy_checkpoint_present"))
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

#[test]
fn operator_status_snapshot_surfaces_repeated_continuation_recovery_lineage() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[tracker::automation_active_label(TEST_SERVICE_ID).as_str()]);
	let worktree_path = config.worktree_root().join("PUB-101");

	for (run_id, attempt_number) in [("run-1", 1), ("run-2", 2)] {
		state_store
			.append_private_execution_event(
				TEST_SERVICE_ID,
				&issue.id,
				run_id,
				attempt_number,
				PHASE_GOAL_RECOVERY_EVENT_TYPE,
				serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": "implement_to_validation_ready",
					"signal": "phase_goal_recovered",
					"payload": {
						"nextPhase": "repair_validation_failures",
						"sourceErrorClass": "app_server_preflight_timeout",
						"sourceErrorMessage": "Timed out while waiting for app-server output.",
					},
				}),
			)
			.expect("phase goal recovery event should record");
	}

	state_store
		.record_run_attempt("run-3", &issue.id, 3, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "run-3", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let recovery = run
		.continuation_recovery
		.as_ref()
		.expect("continuation recovery lineage should project onto current lane");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(recovery.state, "continuation_scheduled");
	assert_eq!(recovery.source_phase, "implement_to_validation_ready");
	assert_eq!(recovery.next_phase, "repair_validation_failures");
	assert_eq!(recovery.source_error_class, "app_server_preflight_timeout");
	assert_eq!(recovery.recovery_count, 2);
	assert_eq!(recovery.automatic_continuation_limit, 1);
	assert!(recovery.budget_exceeded);
	assert_eq!(run.policy_state, "continuation_recovery_churn_exceeded");
	assert!(run.lane_control_conditions.contains(&String::from(
		"continuation_recovery_budget_exceeded"
	)));
	assert!(rendered.contains("continuation_recovery: state=continuation_scheduled"));
	assert!(rendered.contains("count=2/1 budget_exceeded=yes"));
	assert_eq!(
		snapshot_json["current_lanes"][0]["continuation_recovery"]["budget_exceeded"],
		true
	);
}

#[test]
fn operator_status_snapshot_surfaces_phase_acceptance_check() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[tracker::automation_active_label(TEST_SERVICE_ID).as_str()]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"run-1",
			1,
			PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.phase_acceptance_check/1",
				"phase": "implement_to_validation_ready",
				"decision": "fail",
				"reason_code": "no_effective_delta",
				"objective_coverage": { "covered": true },
				"effective_delta": {
					"present": false,
					"changed_surfaces": ["runtime"],
				},
				"non_goal_check": {
					"passed": true,
					"blocker_count": 0,
				},
				"validation_evidence": {
					"repo_gate_passed": true,
				},
				"next_action": "produce an issue-scoped effective delta before completing the phase goal again",
			}),
		)
		.expect("phase acceptance check should record");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, "run-1", "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let acceptance = run
		.phase_acceptance
		.as_ref()
		.expect("phase acceptance should project onto current lane");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(acceptance.decision, "fail");
	assert_eq!(acceptance.reason_code, "no_effective_delta");
	assert_eq!(acceptance.changed_surfaces, vec![String::from("runtime")]);
	assert!(rendered.contains("phase_acceptance: phase=implement_to_validation_ready"));
	assert!(rendered.contains("reason=no_effective_delta"));
}

#[test]
fn operator_status_snapshot_surfaces_merged_dirty_ad_hoc_worktree() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("accounts-column-format");

	git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"xy/accounts-column-format",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);
	commit_worktree_change(&worktree_path, "README.md", "feature work\n", "feature work");
	git_status_success(
		config.repo_root(),
		&["merge", "--no-ff", "xy/accounts-column-format", "-m", "land feature"],
	);

	fs::write(worktree_path.join("README.md"), "dirty after land\n")
		.expect("worktree file should become dirty");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.worktree_path == ".worktrees/accounts-column-format")
		.expect("ad-hoc merged dirty worktree should be surfaced");

	assert!(snapshot.warnings.contains(&String::from("merged_worktree_cleanup_pending")));
	assert!(snapshot.warnings.contains(&String::from("merged_dirty_worktree")));
	assert_eq!(worktree.branch_name, "xy/accounts-column-format");
	assert_eq!(worktree.ownership, "post_land_cleanup");
	assert!(
		worktree
			.ownership_reason
			.contains("already merged into `main`"),
		"ownership reason should explain why the worktree is no longer usable"
	);
	assert!(
		worktree.hygiene.as_ref().is_some_and(|hygiene| hygiene.dirty),
		"hygiene state should mark the local changes"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 1);
	assert_eq!(project.cleanup_pending_count, 0);

	let error = orchestrator::ensure_project_has_no_merged_worktree_cleanup_debt(&config)
		.expect_err("normal automation should stop while merged dirty worktrees remain");

	assert!(error.to_string().contains("Post-land worktree cleanup is pending"));
}

#[test]
fn operator_status_snapshot_explains_unavailable_worktree_hygiene() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	fs::remove_dir_all(config.repo_root().join(".git"))
		.expect("repo metadata should be removable for the fixture");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should degrade instead of failing");
	let detail = snapshot
		.warning_details
		.iter()
		.find(|detail| detail.warning == "worktree_hygiene_unavailable")
		.expect("hygiene warning should include operator-facing detail");

	assert!(snapshot.warnings.contains(&String::from("worktree_hygiene_unavailable")));
	assert_eq!(detail.project_id.as_deref(), Some("pubfi"));

	let repo_root = config.repo_root().display().to_string();

	assert_eq!(detail.repo_root.as_deref(), Some(repo_root.as_str()));
	assert!(detail.reason.contains("not a git repository"));
	assert!(
		detail
			.next_action
			.as_deref()
			.is_some_and(|action| action.contains("Remove the stale project registration")),
		"detail should tell the operator how to clear a stale project registration"
	);

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("project=pubfi"));
	assert!(rendered.contains("repo_root="));
	assert!(rendered.contains("Remove the stale project registration"));
}

#[test]
fn operator_status_snapshot_updates_owned_merged_worktree_hygiene_without_global_warning() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Done", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	git_status_success(
		config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"xy/pub-101-cleanup",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);
	commit_worktree_change(&worktree_path, "README.md", "feature work\n", "feature work");
	git_status_success(
		config.repo_root(),
		&["merge", "--no-ff", "xy/pub-101-cleanup", "-m", "land feature"],
	);

	fs::write(worktree_path.join("README.md"), "dirty after land\n")
		.expect("worktree file should become dirty");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"xy/pub-101-cleanup",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let worktree = snapshot
		.worktrees
		.iter()
		.find(|worktree| worktree.worktree_path == ".worktrees/PUB-101")
		.expect("owned merged worktree should still be visible");

	assert!(!snapshot.warnings.contains(&String::from("merged_worktree_cleanup_pending")));
	assert!(!snapshot.warnings.contains(&String::from("merged_dirty_worktree")));
	assert!(
		worktree.hygiene.as_ref().is_some_and(|hygiene| hygiene.dirty),
		"hygiene should still surface on the owned worktree row"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 1);
	assert_eq!(project.cleanup_pending_count, 0);
}

#[test]
fn live_operator_status_snapshot_hydrates_current_lane_issue_display_metadata() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "xy-392-attempt-1-1777551056";
	let channel_path = temp_dir.path().join("control.channel");
	let mut issue = sample_issue_with_sort_fields(
		"issue-active",
		"XY-392",
		"In Progress",
		&[],
		Some(3),
		"2026-04-30T03:01:00Z",
	);

	issue.title = String::from("Hydrate issue display metadata on run rows");

	git_status_success(
		config.repo_root(),
		&["remote", "add", "origin", "git@github.com:hack-ink/pubfi-mono-v2.git"],
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("run lease should record");
	state_store.update_run_thread(run_id, "thread-1").expect("thread should record");
	state_store.update_run_turn(run_id, "turn-1").expect("turn should record");

	std::fs::write(&channel_path, "ready\n").expect("control channel should write");

	state_store
		.publish_run_control_channel_for_active_attempt(run_id, 1, &channel_path, "local_file")
		.expect("control channel should publish");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let current_lane = snapshot.current_lanes.first().expect("current lane should exist");
	let recent_run = snapshot.recent_runs.first().expect("recent run should exist");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(current_lane.project_id, config.service_id());
	assert_eq!(current_lane.project_display_name, "hack-ink/pubfi-mono-v2");
	assert_eq!(current_lane.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(current_lane.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(current_lane.author.as_deref(), Some("Yvette"));

	let expected_private_evidence_command = format!(
		"decodex evidence --config {} XY-392 --run-id {run_id} --attempt 1 --json",
		config.config_path().display()
	);

	assert_eq!(
		current_lane.private_evidence.read_command,
		expected_private_evidence_command
	);
	assert_eq!(recent_run.issue_identifier.as_deref(), Some("XY-392"));
	assert_eq!(recent_run.title.as_deref(), Some("Hydrate issue display metadata on run rows"));
	assert_eq!(recent_run.author.as_deref(), Some("Yvette"));
	assert_eq!(snapshot_json["current_lanes"][0]["project_id"], "pubfi");
	assert_eq!(
		snapshot_json["current_lanes"][0]["project_display_name"],
		"hack-ink/pubfi-mono-v2"
	);
	assert_eq!(snapshot_json["current_lanes"][0]["issue_identifier"], "XY-392");
	assert_eq!(
		snapshot_json["current_lanes"][0]["title"],
		"Hydrate issue display metadata on run rows"
	);
	assert_eq!(snapshot_json["current_lanes"][0]["author"], "Yvette");
	assert_eq!(
		snapshot_json["current_lanes"][0]["private_evidence"]["read_command"],
		expected_private_evidence_command
	);
	assert_eq!(snapshot_json["current_lanes"][0]["control_capability"]["status"], "active");
	assert_eq!(
		snapshot_json["current_lanes"][0]["control_capability"]["thread_id"],
		"thread-1"
	);
	assert_eq!(
		snapshot_json["current_lanes"][0]["control_capability"]["turn_id"],
		"turn-1"
	);
}

#[test]
fn idle_operator_status_snapshot_has_no_runtime_or_recovery_noise() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("idle snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(snapshot.project_id, "pubfi");
	assert_eq!(snapshot.run_limit, 10);
	assert!(snapshot.warnings.is_empty(), "idle snapshot warnings: {:?}", snapshot.warnings);
	assert!(snapshot.current_lanes.is_empty(), "idle snapshot should have no current lanes");
	assert!(snapshot.recent_runs.is_empty(), "idle snapshot should have no run history");
	assert!(snapshot.history_lanes.is_empty(), "idle snapshot should have no run ledger lanes");
	assert!(
		snapshot.queued_candidates.is_empty(),
		"idle snapshot should have no queued candidates"
	);
	assert!(snapshot.worktrees.is_empty(), "idle snapshot should have no recovery worktrees");
	assert!(
		snapshot.post_review_lanes.is_empty(),
		"idle snapshot should have no retained post-review lanes"
	);

	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(project.retained_worktree_count, 0);
	assert_eq!(project.waiting_lane_count, 0);
	assert_eq!(project.attention_count, 0);
	assert_eq!(project.cleanup_blocked_count, 0);
	assert_eq!(project.cleanup_pending_count, 0);
	assert_eq!(project.connector_state, "ok");
	assert_eq!(project.last_activity_at, None);

	for field in [
		"warnings",
		"warning_details",
		"current_lanes",
		"recent_runs",
		"history_lanes",
		"queued_candidates",
		"worktrees",
		"post_review_lanes",
	] {
		assert_eq!(
			snapshot_json[field],
			serde_json::json!([]),
				"idle operator snapshot field {field} should serialize as an empty array",
		);
	}

	assert!(rendered.contains("Warnings: 0"));
	assert!(rendered.contains("Running lanes: 0"));
	assert!(rendered.contains("Run ledger shown: 0 issue lanes from 0 history attempts"));
	assert!(rendered.contains("Backlog: 0"));
	assert!(rendered.contains("Claimed queue echoes: 0"));
	assert!(rendered.contains("Stale closed queue labels: 0"));
	assert!(rendered.contains("Recovery worktrees: 0"));
	assert!(rendered.contains("Post-review lanes: 0"));
	assert!(rendered.contains("\nCurrent Lanes\n- none\n"));
	assert!(rendered.contains("\nRun Ledger\n- none\n"));
	assert!(rendered.contains("\nBacklog\n- none\n"));
	assert!(rendered.contains("\nClaimed Queue Echoes\n- none\n"));
	assert!(rendered.contains("\nStale Closed Queue Labels\n- none\n"));
	assert!(rendered.contains("\nRecovery Worktrees\n- none\n"));
	assert!(rendered.contains("\nPost-Review Lanes\n- none\n"));
	assert!(!rendered.contains("Warning details:"));
	assert!(!rendered.contains("run_id:"));
	assert!(!rendered.contains("run_lease: true"));
	assert!(!rendered.contains("role: post_review_lane"));
	assert!(!rendered.contains("role: cleanup_only"));
}

#[test]
fn idle_operator_status_snapshot_includes_configured_codex_accounts() {
	let (temp_dir, base_config, _workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let accounts_path = temp_dir.path().join(".codex/decodex/accounts.jsonl");
	let usage_endpoint = start_codex_usage_fixture_server(vec![
		(
			"acct_default",
			r#"{"plan_type":"pro","rate_limit":{"primary_window":{"used_percent":7,"limit_window_seconds":18000,"reset_at":1800018000},"secondary_window":{"used_percent":11,"limit_window_seconds":604800,"reset_at":1800604800}},"credits":{"has_credits":true,"unlimited":false,"balance":"12.34"}}"#,
		),
		(
			"acct_copy",
			r#"{"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":22,"limit_window_seconds":18000,"reset_at":1800019000},"secondary_window":{"used_percent":33,"limit_window_seconds":604800,"reset_at":1800605800}},"credits":{"has_credits":false,"unlimited":false,"balance":"0"}}"#,
		),
	]);

	std::fs::create_dir_all(accounts_path.parent().expect("accounts path should have parent"))
		.expect("accounts dir should exist");
	std::fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
{"email":"copy@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-copy","refresh_token":"refresh-copy","account_id":"acct_copy"}}
"#,
	)
	.expect("accounts fixture should write");

	let mut config_toml = service_config_toml_for_config(
		&base_config,
		base_config.github().token_env_var(),
		base_config.codex().review_level(),
	);

	config_toml.push_str(&format!(
		"\n[codex.accounts]\nusage_endpoint = \"{}\"\n",
		usage_endpoint
	));

	write_service_config(base_config.repo_root(), &config_toml);

	let config = load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let accounts =
		snapshot_json["accounts"].as_array().expect("snapshot should expose configured accounts");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot_json["account_control"]["mode"], "balanced");
	assert_eq!(
		snapshot_json["account_control"]["account_selector"],
		serde_json::Value::Null,
	);
	assert_eq!(accounts.len(), 2);
	assert_eq!(accounts[0]["email"], "default@example.com");
	assert_eq!(accounts[0]["status"], "available");
	assert_eq!(accounts[0]["refresh_status"], "not_needed");
	assert_eq!(accounts[0]["plan_type"], "pro");
	assert_eq!(accounts[0]["primary_remaining_percent"], 93);
	assert_eq!(accounts[0]["credits_balance"], "12.34");
	assert_eq!(accounts[1]["email"], "copy@example.com");
	assert_eq!(accounts[1]["status"], "available");
	assert_eq!(accounts[1]["refresh_status"], "not_needed");
	assert_eq!(accounts[1]["plan_type"], "plus");
	assert_eq!(accounts[1]["primary_remaining_percent"], 78);
	assert_eq!(accounts[1]["credits_balance"], "0");
}

#[test]
fn status_command_snapshot_does_not_probe_configured_codex_accounts() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let accounts_path = temp_dir.path().join(".codex/decodex/accounts.jsonl");

	std::fs::create_dir_all(accounts_path.parent().expect("accounts path should have parent"))
		.expect("accounts dir should exist");
	std::fs::write(
		&accounts_path,
		r#"{"email":"default@example.com","auth_mode":"chatgpt","tokens":{"access_token":"access-default","refresh_token":"refresh-default","account_id":"acct_default"}}
"#,
	)
	.expect("accounts fixture should write");

	let mut config_toml = service_config_toml_for_config(
		&base_config,
		base_config.github().token_env_var(),
		base_config.codex().review_level(),
	);

	config_toml.push_str(
		"\n[codex.accounts]\nusage_endpoint = \"http://127.0.0.1:9/wham/usage\"\n",
	);

	write_service_config(base_config.repo_root(), &config_toml);

	let config = load_service_config(base_config.repo_root());
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_status_command_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status command snapshot should build without probing account usage");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let accounts =
		snapshot_json["accounts"].as_array().expect("snapshot should expose configured accounts");

	assert_eq!(accounts.len(), 1);
	assert_eq!(accounts[0]["email"], "default@example.com");
	assert_eq!(accounts[0]["status"], "available");
	assert_eq!(accounts[0]["refresh_status"], "not_checked");
	assert_eq!(accounts[0]["primary_remaining_percent"], serde_json::Value::Null);
	assert!(!snapshot.warnings.contains(&String::from("codex_accounts_unavailable")));
}

fn start_codex_usage_fixture_server(responses: Vec<(&'static str, &'static str)>) -> String {
	let listener = TcpListener::bind("127.0.0.1:0").expect("usage fixture server should bind");
	let address = listener.local_addr().expect("usage fixture address should resolve");

	thread::spawn(move || {
		let responses_by_account =
			responses.into_iter().collect::<HashMap<_, _>>();
		let request_count = responses_by_account.len();

		for _ in 0..request_count {
			let (mut stream, _peer) =
				listener.accept().expect("usage fixture request should arrive");
			let mut request = [0_u8; 4_096];
			let bytes_read = stream.read(&mut request).expect("usage request should read");
			let request = String::from_utf8_lossy(&request[..bytes_read]);
			let account_id = usage_fixture_account_id(&request);
			let (status, body) = match account_id.and_then(|account_id| {
				responses_by_account.get(account_id).copied()
			}) {
				Some(body) => ("200 OK", body),
				None => ("404 Not Found", r#"{"error":"unknown account"}"#),
			};
			let response = format!(
				"HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
				body.len(),
				body
			);

			stream
				.write_all(response.as_bytes())
				.expect("usage fixture response should write");

			let _ = stream.shutdown(Shutdown::Both);
		}
	});

	format!("http://{address}/wham/usage")
}

fn usage_fixture_account_id(request: &str) -> Option<&str> {
	request.lines().find_map(|line| {
		let (name, value) = line.split_once(':')?;

		name.eq_ignore_ascii_case("ChatGPT-Account-Id").then_some(value.trim())
	})
}

#[test]
fn operator_status_snapshot_includes_local_recovery_worktree_directories() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join("PUB-199");

	fs::create_dir_all(&worktree_path).expect("worktree directory should exist");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert_eq!(snapshot.worktrees.len(), 1);
	assert_eq!(snapshot.worktrees[0].issue_id, "PUB-199");
	assert!(!snapshot.worktrees[0].branch_name.is_empty());
	assert_eq!(snapshot.worktrees[0].worktree_path, ".worktrees/PUB-199");
	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert!(snapshot.worktrees[0].ownership_reason.contains("local cleanup only"));
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
}

#[test]
fn completed_retained_worktree_without_post_review_owner_is_cleanup_only() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-cleanup",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-199",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert!(snapshot.post_review_lanes.is_empty());
	assert_eq!(snapshot.projects[0].retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].issue_identifier.as_deref(), Some("PUB-199"));
	assert_eq!(snapshot.worktrees[0].issue_state.as_deref(), Some("Done"));
	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert!(snapshot.worktrees[0].ownership_reason.contains("Issue is Done"));
	assert_eq!(snapshot_json["worktrees"][0]["ownership"], "cleanup_only");
	assert_eq!(snapshot_json["worktrees"][0]["issue_state"], "Done");
	assert!(rendered.contains("role: cleanup_only"));
	assert!(rendered.contains("reason: Issue is Done"));
	assert!(!rendered.contains("role: post_review_lane"));
	assert!(!rendered.contains("classification: blocked"));
	assert!(!rendered.contains("review_handoff_missing"));
}

#[test]
fn legacy_cleanup_only_worktree_requires_audited_manual_closeout() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let db_path = temp_dir.path().join("legacy-runtime.sqlite3");
	let issue = sample_issue_with_sort_fields(
		"issue-cleanup",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(&format!(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('{}', 'pubfi', 'x/pubfi-pub-199', '{}');",
				issue.id,
				worktree_path.display()
			))
			.expect("legacy worktree row should write");
	}

	let tracker = FakeTracker::new(vec![issue]);
	let state_store = StateStore::open(&db_path).expect("state store should migrate");
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");

	assert_eq!(snapshot.worktrees[0].ownership, "cleanup_only");
	assert_eq!(snapshot.worktrees[0].provenance.source, "legacy_unknown");
	assert!(snapshot.worktrees[0].provenance.audit_required);
	assert!(
		snapshot.worktrees[0]
			.recovery_next_action
			.as_deref()
			.is_some_and(|action| action.contains("decodex recover legacy-closeout PUB-199"))
	);
	assert_eq!(snapshot_json["worktrees"][0]["provenance"]["source"], "legacy_unknown");
	assert_eq!(snapshot_json["worktrees"][0]["provenance"]["audit_required"], true);
	assert!(rendered.contains("provenance_source: legacy_unknown"));
	assert!(rendered.contains("audit_required: true"));
	assert!(rendered.contains("recovery_next_action: verify tracker/PR terminal state"));
	assert!(rendered.contains("decodex recover legacy-closeout PUB-199"));
}

#[test]
fn runtime_recovery_preserves_legacy_cleanup_only_provenance_without_recoverable_owner() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let db_path = temp_dir.path().join("runtime.sqlite3");
	let (_layout_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue_with_sort_fields(
		"issue-legacy",
		"PUB-199",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("legacy worktree path should exist");

	{
		let connection = Connection::open(&db_path).expect("legacy db should open");

		connection
			.execute_batch(&format!(
				"CREATE TABLE worktrees (
					issue_id TEXT PRIMARY KEY NOT NULL,
					project_id TEXT NOT NULL,
					branch_name TEXT NOT NULL,
					worktree_path TEXT NOT NULL
				);
				INSERT INTO worktrees (issue_id, project_id, branch_name, worktree_path)
				VALUES ('{}', 'pubfi', 'x/pubfi-pub-199', '{}');",
				issue.id,
				worktree_path.display()
			))
			.expect("legacy worktree row should write");
	}

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open(&db_path).expect("state store should migrate");
	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");
	let mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("legacy mapping should remain");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"terminal cleanup-only worktree should not become a retry lane"
	);
	assert_eq!(mapping.provenance().source(), "legacy_unknown");
	assert_eq!(mapping.provenance().created_at_unix(), None);
	assert_eq!(mapping.provenance().updated_at_unix(), None);
}

#[test]
fn runtime_recovery_records_recovered_provenance_for_fresh_active_worktree() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_active_issue("In Progress");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("activity marker should load")
		.expect("activity marker should exist");
	let observed_at_unix = marker
		.last_activity_unix_epoch()
		.expect("activity marker should have a stable timestamp");
	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");
	let mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("recovered mapping should exist");
	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("fresh active marker should recover the lease");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh marker should recover as the run lease instead of a retry queue item"
	);
	assert_eq!(mapping.provenance().source(), "runtime_recovered");
	assert_eq!(mapping.provenance().created_at_unix(), Some(observed_at_unix));
	assert_eq!(mapping.provenance().updated_at_unix(), Some(observed_at_unix));
	assert_eq!(lease.run_id(), "run-1");
}

#[test]
fn runtime_recovery_splits_invalid_local_id_batch_without_losing_valid_issue() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let mut issue = sample_active_issue("In Progress");

	issue.id = String::from("00000000-0000-0000-0000-000000000101");

	let tracker = FakeTracker::with_refresh_error(
		vec![issue.clone()],
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("active worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-101", 1)
		.expect("activity marker should write");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("valid worktree mapping should record");
	state_store
		.record_run_attempt("run-12", "PUB-012", 1, "running")
		.expect("invalid local run attempt should record");
	state_store
		.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
		.expect("invalid local lease should record");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should split invalid local ids from valid server ids");
	let recovered_mapping = state_store
		.worktree_for_issue(&issue.id)
		.expect("mapping lookup should succeed")
		.expect("valid issue mapping should remain");
	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("valid issue lease should recover");

	assert!(
		recovered_state.recoverable_issues.is_empty(),
		"fresh valid issue should recover as active lease rather than disappear"
	);
	assert_eq!(recovered_mapping.issue_id(), issue.id);
	assert_eq!(lease.issue_id(), issue.id);
	assert_eq!(lease.run_id(), "run-101");
}

#[test]
fn operator_status_snapshot_reports_retry_backoff_from_worktree_marker() {
	for (retry_kind, expected_wait_reason) in
		[("failure", "failure_retry"), ("git_lock_contention", "git_lock_contention")]
	{
		let (_temp_dir, config, _workflow) = temp_project_layout();
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = sample_issue("Todo", &[]);
		let worktree_path = config.worktree_root().join("PUB-101");

		state_store
			.record_run_attempt("run-1", &issue.id, 1, "failed")
			.expect("run attempt should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		state::write_run_retry_schedule(
			&worktree_path,
			"run-1",
			1,
			retry_kind,
			OffsetDateTime::now_utc().unix_timestamp() + 60,
		)
		.expect("retry schedule marker should write");

		let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
			.expect("snapshot should build");
		let run = snapshot.recent_runs.first().expect("recent run should exist");

		assert_eq!(run.phase, "retry_backoff");
		assert_eq!(run.wait_reason.as_deref(), Some(expected_wait_reason));
		assert_eq!(run.retry_kind.as_deref(), Some(retry_kind));
		assert!(run.next_retry_at.is_some());
		assert_eq!(snapshot.projects[0].waiting_lane_count, 1);
		assert_eq!(snapshot.projects[0].connector_state, "backoff");
	}
}

#[test]
fn operator_status_snapshot_ignores_retry_schedule_on_running_attempt() {
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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("run marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-1",
		1,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("stale retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.wait_reason, None);
	assert_eq!(run.retry_kind, None);
	assert_eq!(run.next_retry_at, None);
	assert_eq!(snapshot.projects[0].waiting_lane_count, 0);
	assert_eq!(snapshot.projects[0].connector_state, "ok");
}

#[test]
fn operator_status_snapshot_reports_stalled_runs_explicitly() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
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

	assert_eq!(run.phase, "stalled");
	assert_eq!(run.wait_reason.as_deref(), Some("app_server_idle_timeout"));
	assert_eq!(run.current_operation, state::RUN_OPERATION_IDLE);
	assert!(!run.suspected_stall);
}

#[test]
fn operator_status_snapshot_surfaces_reconciliation_operation_for_stalled_runs() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_operation_marker(
		&worktree_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	)
	.expect("reconciliation marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.phase, "stalled");
	assert_eq!(run.current_operation, state::RUN_OPERATION_RECONCILIATION);
}

#[test]
fn operator_status_snapshot_preserves_stalled_run_activity_when_tagging_reconciliation() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, 42)
		.expect("initial activity marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let stale_activity = OffsetDateTime::now_utc().unix_timestamp() - 600;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_activity_unix_epoch=") {
				format!("last_activity_unix_epoch={stale_activity}")
			} else if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_activity}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");
	state::write_run_operation_marker_preserving_activity(
		&worktree_path,
		"run-1",
		1,
		RUN_OPERATION_RECONCILIATION,
	)
	.expect("reconciliation marker should preserve existing activity");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("marker snapshot should load")
		.expect("marker snapshot should exist");
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(marker.process_id(), Some(42));
	assert_eq!(marker.last_activity_unix_epoch(), Some(stale_activity));
	assert_eq!(run.current_operation, state::RUN_OPERATION_RECONCILIATION);
	assert_eq!(run.process_id, Some(42));
}

#[test]
fn operator_status_snapshot_marks_soft_stalls_before_hard_timeout() {
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

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let suspected_age = (RUN_LEASE_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - suspected_age;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_progress}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.current_operation, state::RUN_OPERATION_AGENT_RUN);
	assert!(run.last_progress_at.is_some());
	assert!(run.suspected_stall);
}

#[test]
fn operator_status_snapshot_diagnoses_protocol_only_model_execution() {
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

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");

	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = fs::read_to_string(&marker_path).expect("marker body should load");
	let suspected_age = (MODEL_EXECUTION_IDLE_TIMEOUT.as_secs() / 2).saturating_add(1) as i64;
	let stale_progress = OffsetDateTime::now_utc().unix_timestamp() - suspected_age;
	let rewritten = marker_body
		.lines()
		.map(|line| {
			if line.starts_with("last_progress_unix_epoch=") {
				format!("last_progress_unix_epoch={stale_progress}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
		+ "\n";

	fs::write(&marker_path, rewritten).expect("marker body should rewrite");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("model_execution")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/status/changed"),
				category: String::from("thread"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("thread/goal/updated"),
				category: String::from("protocol"),
				detail: Some(String::from("active")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("account/rateLimits/updated"),
				category: String::from("rate_limit"),
				detail: Some(String::from("pro")),
			},
		],
		..ProtocolActivitySummary::default()
	};

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "account/rateLimits/updated",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol-only marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.wait_reason.as_deref(), Some("model_execution"));
	assert_eq!(run.progress_diagnostic.as_deref(), Some("protocol_only_activity"));
	assert_eq!(run.execution_liveness, "process_alive");
	assert!(run.suspected_stall);
	assert_ne!(run.last_progress_at, run.last_protocol_activity_at);
	assert!(rendered.contains("progress_diagnostic: protocol_only_activity"));
}

#[test]
fn operator_status_snapshot_counts_stopped_active_process_as_attention_not_running() {
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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, u32::MAX)
		.expect("stopped process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[cfg(unix)]
#[test]
fn operator_status_snapshot_counts_zombie_active_process_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let mut child = Command::new("/bin/sh")
		.arg("-c")
		.arg("exit 0")
		.spawn()
		.expect("short-lived child process should spawn");
	let child_pid = child.id();

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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, child_pid)
		.expect("zombie process marker should write");

	let deadline = Instant::now() + Duration::from_secs(5);
	let mut observed_stopped = false;

	while Instant::now() < deadline {
		if !orchestrator::process_is_alive(child_pid) {
			observed_stopped = true;

			break;
		}

		thread::sleep(Duration::from_millis(10));
	}

	let snapshot = observed_stopped.then(|| {
		orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
			.expect("snapshot should build")
	});

	child.wait().expect("short-lived child process should reap");

	assert!(observed_stopped, "exited child process must not count as alive");

	let snapshot = snapshot.expect("snapshot should be captured while child is unreaped");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_status_snapshot_counts_previous_boot_process_as_attention_not_running() {
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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live process marker should write");

	rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_id, Some(process::id()));
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.execution_liveness, "process_identity_mismatch");
	assert_eq!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch"));
	assert!(!run.has_fresh_execution);
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_status_snapshot_projects_unleased_app_server_current_lane_as_retained_attention() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

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

	rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "running");
	assert_eq!(run.phase, "executing");
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "process_identity_mismatch");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch"));
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.liveness_state, "host_boot_mismatch");
	assert_eq!(run.lane_control_next_action, "inspect_recovery_evidence");
	assert!(run
		.lane_control_conditions
		.iter()
		.any(|condition| condition == "host_boot_id_mismatch"));
	assert!(run.has_fresh_execution);
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "retained_attention");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_status_snapshot_does_not_shadow_post_review_lane_with_retained_attention_run() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

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

	rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let mut snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	snapshot.post_review_lanes = vec![orchestrator::OperatorPostReviewLaneStatus {
		project_id: String::from("pubfi"),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: String::from("In Review"),
		branch_name: String::from("x/pubfi-pub-101"),
		worktree_path: String::from(".worktrees/PUB-101"),
		classification: String::from("blocked"),
		reason: String::from("review_handoff_lineage_mismatch"),
		pr_url: Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/101")),
		pr_head_sha: Some(String::from("1111111111111111111111111111111111111111")),
		pr_state: Some(String::from("OPEN")),
		review_decision: Some(String::from("CHANGES_REQUESTED")),
		mergeable: Some(String::from("UNKNOWN")),
		check_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: Some(1),
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: Some(String::from("lineage_validation_failed")),
		loop_status: None,
	}];

	orchestrator::hydrate_post_review_lane_current_lane_shadowing(&mut snapshot);
	orchestrator::refresh_operator_project_summary(&mut snapshot, None);

	let project = snapshot.projects.first().expect("project summary should exist");
	let lane = snapshot.post_review_lanes.first().expect("post-review lane should remain visible");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes[0].has_fresh_execution);
	assert!(!snapshot.current_lanes[0].counts_as_running);
	assert!(snapshot.current_lanes[0].needs_attention);
	assert_eq!(snapshot.current_lanes[0].ownership_state, "retained_attention");
	assert!(!lane.shadowed_by_current_lane);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.post_review_lane_count, 1);
	assert_eq!(project.waiting_lane_count, 0);
	assert_eq!(project.attention_count, 1);
	assert!(rendered.contains("shadowed_by_current_lane: no"));
	assert!(rendered.contains("readback_root_cause: lineage_validation_failed"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_status_snapshot_counts_reused_pid_as_attention_not_running() {
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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live process marker should write");

	rewrite_run_activity_marker_process_start_identity(&worktree_path, "previous-process-start");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_id, Some(process::id()));
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.execution_liveness, "process_identity_mismatch");
	assert_eq!(
		run.process_liveness_reason.as_deref(),
		Some("process_start_identity_mismatch")
	);
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[test]
fn operator_status_snapshot_keeps_unleased_live_process_visible_but_not_running() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "running");
	assert_eq!(run.attempt_status, "running");
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "process_alive");
	assert_eq!(run.process_alive, Some(true));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_alive"));
	assert_eq!(run.ownership_state, "orphaned_live_thread");
	assert_eq!(run.liveness_state, "process_alive");
	assert_eq!(
		run.lane_control_next_action,
		"inspect_or_interrupt_orphaned_live_thread"
	);
	assert!(run
		.lane_control_conditions
		.iter()
		.any(|condition| condition == "run_lease_missing"));
	assert!(run.has_fresh_execution);
	assert!(!run.counts_as_running);
	assert!(!run.needs_attention);
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "orphaned_live_thread");
}

#[test]
fn operator_status_snapshot_keeps_terminal_status_live_process_in_recent_orphan_bucket() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("live terminal run should remain inspectable");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "failed");
	assert_eq!(run.attempt_status, "failed");
	assert_eq!(run.status_projection_reason, None);
	assert_eq!(run.phase, "failed");
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert_eq!(run.ownership_state, "orphaned_live_thread");
	assert_eq!(run.liveness_state, "process_alive");
	assert_eq!(run.terminalization_state, "barrier_started");
	assert!(run
		.lane_control_conditions
		.iter()
		.any(|condition| condition == "terminal_attempt_has_live_evidence"));
	assert_eq!(run.process_alive, Some(true));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_alive"));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "orphaned_live_thread");
}

#[test]
fn operator_status_snapshot_excludes_terminal_thread_archive_from_running_lanes() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.append_event("run-1", 1, "thread/archive", "{}")
		.expect("thread archive event should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(
		snapshot.current_lanes.is_empty(),
		"terminal archive-only protocol events must not present as active execution"
	);
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert!(
		snapshot.recent_runs.iter().all(|run| run.run_id != "run-1"),
		"archive-only terminal attempts do not need to remain operator-visible"
	);
}

#[test]
fn operator_status_snapshot_projects_terminal_run_with_active_thread_as_retained_attention() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "stalled")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
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

	rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("terminal run should remain inspectable");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.status, "stalled");
	assert_eq!(run.attempt_status, "stalled");
	assert_eq!(run.status_projection_reason, None);
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert_eq!(run.ownership_state, "retained_attention");
	assert_eq!(run.liveness_state, "host_boot_mismatch");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("host_boot_id_mismatch"));
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert!(run
		.lane_control_conditions
		.iter()
		.any(|condition| condition == "host_boot_id_mismatch"));
}

#[test]
fn operator_status_snapshot_keeps_succeeded_status_live_process_in_recent_orphan_bucket() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "succeeded")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, process::id())
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("live succeeded run should remain inspectable");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.status, "succeeded");
	assert_eq!(run.attempt_status, "succeeded");
	assert_eq!(run.status_projection_reason, None);
	assert_eq!(run.phase, "completed");
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert_eq!(run.ownership_state, "orphaned_live_thread");
	assert_eq!(run.liveness_state, "process_alive");
	assert_eq!(run.terminalization_state, "barrier_started");
	assert_eq!(run.process_alive, Some(true));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_alive"));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "orphaned_live_thread");
}

#[test]
fn operator_status_projects_terminal_finalized_run_as_pending_not_active() {
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

	state_store.upsert_project(&registration).expect("project should register");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.update_run_thread("run-1", "thread-1").expect("thread should record");
	state_store.update_run_turn("run-1", "turn-1").expect("turn should record");
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
		.append_event("run-1", 1, "skills/changed", "{}")
		.expect("protocol event should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"terminal_finalize",
			serde_json::json!({
				"path": "review_handoff",
				"mode": "handoff",
				"branch": "x/pubfi-pub-101",
				"worktree_path": worktree_path.display().to_string(),
			}),
		)
		.expect("terminal finalize event should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	let mut child = Command::new("sleep").arg("30").spawn().expect("sleep child should start");
	let child_pid = child.id();

	state::write_run_activity_marker_for_process(&worktree_path, "run-1", 1, child_pid)
		.expect("live process marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert_terminal_pending_status_projection(&snapshot);
	assert_terminal_pending_lane_inspect(&state_store);
	assert_terminal_pending_interrupt_rejects_force(&state_store);

	if matches!(child.try_wait(), Ok(None)) {
		child.kill().expect("sleep child should be killable");
	}

	child.wait().expect("sleep child should reap");
}

fn assert_terminal_pending_status_projection(snapshot: &OperatorStatusSnapshot) {
	let project = snapshot.projects.first().expect("project summary should exist");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("terminal-pending run should remain inspectable in recent runs");

	assert!(
		snapshot.current_lanes.is_empty(),
		"terminal-finalized runs must not keep presenting as active execution"
	);
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(run.status, "review_handoff_pending");
	assert_eq!(run.attempt_status, "running");
	assert_eq!(run.phase, "terminal_pending");
	assert_eq!(run.wait_reason.as_deref(), Some("review_handoff_writeback"));
	assert_eq!(run.current_operation, state::RUN_OPERATION_REVIEW_WRITEBACK);
	assert!(!run.run_lease);
	assert_eq!(run.queue_lease_state, "not_held");
	assert_eq!(run.execution_liveness, "not_running");
	assert!(!run.suspected_stall);
	assert_eq!(run.last_event_type.as_deref(), Some("skills/changed"));
	assert_eq!(
		run.loop_status.as_ref().map(|status| status.summary.as_str()),
		Some("terminal lifecycle: review_handoff_pending")
	);
}

fn assert_terminal_pending_lane_inspect(state_store: &StateStore) {
	let response = String::from_utf8(orchestrator::build_operator_lane_inspect_http_response(
		state_store,
		format!(
			"GET {}?projectId=pubfi&issue=PUB-101&runId=run-1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
			orchestrator::OPERATOR_LANE_INSPECT_ENDPOINT_PATH
		)
		.as_bytes(),
	))
	.expect("lane inspect response should be utf-8");
	let data = operator_status_response_json_body(&response, "lane inspect");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(data["matchedRunCount"], 1);
	assert_eq!(data["runs"][0]["status"], "review_handoff_pending");
	assert_eq!(data["runs"][0]["phase"], "terminal_pending");
	assert_eq!(data["runs"][0]["waitReason"], "review_handoff_writeback");
	assert_eq!(data["runs"][0]["currentOperation"], state::RUN_OPERATION_REVIEW_WRITEBACK);
	assert_eq!(data["runs"][0]["runLease"], false);
	assert_eq!(data["runs"][0]["executionLiveness"], "not_running");
	assert_eq!(data["runs"][0]["softInterruptAvailable"], false);
	assert_eq!(data["runs"][0]["hardInterruptAvailable"], false);
}

fn assert_terminal_pending_interrupt_rejects_force(state_store: &StateStore) {
	let body = br#"{"projectId":"pubfi","issue":"PUB-101","runId":"run-1","force":true}"#;
	let response = String::from_utf8(orchestrator::build_operator_lane_interrupt_http_response(
		state_store,
		format!(
			"POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
			orchestrator::OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
			body.len(),
			String::from_utf8_lossy(body)
		)
		.as_bytes(),
	))
	.expect("lane interrupt response should be utf-8");
	let data = operator_status_response_json_body(&response, "lane interrupt");

	assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
	assert_eq!(data["classification"], "soft_interrupt_unavailable");
	assert_eq!(data["softInterrupt"]["errorClass"], "lane_not_active");
	assert_eq!(data["hardInterrupt"], Value::Null);
}

fn operator_status_response_json_body(response: &str, context: &str) -> Value {
	let body = response
		.split_once("\r\n\r\n")
		.map(|(_, body)| body)
		.unwrap_or_else(|| panic!("{context} response should include body"));

	serde_json::from_str(body).unwrap_or_else(|_| panic!("{context} response should be json"))
}

#[test]
fn operator_status_snapshot_promotes_starting_after_app_server_activity() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
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
	state::write_run_effective_runtime_marker(
		&worktree_path,
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "workspaceWrite",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
				event_count: 2,
				last_event_type: "model/response",
				child_agent_activity: None,
				protocol_activity: None,
			},
		)
	.expect("protocol summary should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.status, "running");
	assert_eq!(run.attempt_status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.queue_lease_state, "held");
	assert_eq!(run.execution_liveness, "process_alive");
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert_eq!(run.effective_model.as_deref(), Some("gpt-5.4"));
	assert!(rendered.contains("status: running"));
	assert!(rendered.contains("attempt_status: starting"));
}

#[test]
fn operator_status_snapshot_counts_stale_starting_run_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "starting")
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

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id=run-1\nattempt_number=1\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nlast_progress_unix_epoch={stale_activity}\n"
		),
	)
	.expect("stale processless marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.status, "starting");
	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_alive, None);
	assert!(run.protocol_idle_for_seconds.is_some_and(|idle| {
		u64::try_from(idle).is_ok_and(|idle| idle >= RUN_LEASE_IDLE_TIMEOUT.as_secs())
	}));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[test]
fn operator_status_snapshot_shadows_stale_attempt_when_newer_leased_attempt_exists() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;
	let stale_run_id = "pub-101-attempt-2-1781621836";
	let current_run_id = "pub-101-attempt-3-1781623863";

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 2, "running")
		.expect("stale run attempt should record");
	state_store
		.record_run_attempt(current_run_id, &issue.id, 3, "running")
		.expect("current run attempt should record");
	state_store
		.upsert_lease("pubfi", &issue.id, current_run_id, "In Progress")
		.expect("current run lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id={stale_run_id}\nattempt_number=2\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nevent_count=1\nlast_event_type=skills/changed\n"
		),
	)
	.expect("stale protocol marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, current_run_id);
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == stale_run_id));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 1);
	assert_eq!(project.attention_count, 0);
	assert!(rendered.contains("Current lanes: 1"));
	assert!(rendered.contains("Running lanes: 1"));
	assert!(!rendered.contains(&format!("- run_id: {stale_run_id}")));
	assert!(
		rendered.contains(&format!("lifecycle_evidence: run={stale_run_id}")),
		"shadowed attempts should remain available only in lifecycle evidence"
	);
}

#[test]
fn operator_status_snapshot_shadows_stale_attempt_when_newer_attempt_has_released_lease() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let stale_activity =
		OffsetDateTime::now_utc().unix_timestamp() - RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 - 30;
	let stale_run_id = "pub-101-attempt-2-1781621836";
	let newer_run_id = "pub-101-attempt-3-1781623863";

	state_store
		.record_run_attempt(stale_run_id, &issue.id, 2, "running")
		.expect("stale run attempt should record");
	state_store
		.record_run_attempt(newer_run_id, &issue.id, 3, "succeeded")
		.expect("newer run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		format!(
			"run_id={stale_run_id}\nattempt_number=2\nlast_activity_unix_epoch={stale_activity}\nlast_protocol_activity_unix_epoch={stale_activity}\nevent_count=1\nlast_event_type=skills/changed\n"
		),
	)
	.expect("stale protocol marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert!(snapshot.current_lanes.is_empty());
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == stale_run_id));
	assert!(snapshot.recent_runs.iter().any(|run| run.run_id == newer_run_id));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 0);
}

#[test]
fn operator_status_snapshot_excludes_completed_lingering_lease_from_current_lanes() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let completed_issue = sample_issue_with_sort_fields(
		"issue-1",
		"XY-379",
		"Done",
		&[],
		Some(3),
		"2026-04-29T17:00:33.133Z",
	);
	let active_issue = sample_issue_with_sort_fields(
		"issue-2",
		"XY-378",
		"In Progress",
		&[],
		Some(3),
		"2026-04-29T17:01:33.133Z",
	);
	let completed_run_id = "xy-379-attempt-1-1777482033";
	let current_lane_run_id = "xy-378-attempt-1-1777482000";

	state_store
		.record_run_attempt(completed_run_id, &completed_issue.id, 1, "running")
		.expect("completed run should record");
	state_store
		.upsert_lease("pubfi", &completed_issue.id, completed_run_id, "In Progress")
		.expect("stale run lease should remain in runtime db");
	state_store
		.append_event(completed_run_id, 1, "turn/completed", "{\"turn\":\"1\"}")
		.expect("terminal protocol evidence should record");
	state_store
		.update_run_status(completed_run_id, "succeeded")
		.expect("terminal status should update");
	state_store
		.record_run_attempt(current_lane_run_id, &active_issue.id, 1, "running")
		.expect("current lane should record");
	state_store
		.upsert_lease("pubfi", &active_issue.id, current_lane_run_id, "In Progress")
		.expect("run lease should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let completed_run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == completed_run_id)
		.expect("completed stale-lease run should remain in history");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].run_id, current_lane_run_id);
	assert_eq!(snapshot.current_lanes[0].phase, "executing");
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(completed_run.phase, "completed");
	assert!(
		completed_run.run_lease,
		"regression setup should keep the stale lease visible in history"
	);
}

#[test]
fn operator_status_snapshot_rolls_current_child_bucket_elapsed_time_into_bucket() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let started_at = OffsetDateTime::now_utc().unix_timestamp() - 90;

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
			event_count: 1,
			last_event_type: "item/tool/call",
			child_agent_activity: Some(&ChildAgentActivitySummary {
				buckets: vec![state::ChildAgentActivityBucket {
					name: String::from("Tracker"),
					event_count: 1,
					tool_call_count: 1,
					..state::ChildAgentActivityBucket::default()
				}],
				current_bucket: Some(String::from("Tracker")),
				current_detail: Some(String::from("issue_progress_checkpoint")),
				current_started_unix_epoch: Some(started_at),
				current_elapsed_seconds: Some(0),
				event_count: 1,
				tool_call_count: 1,
				..ChildAgentActivitySummary::default()
				}),
				protocol_activity: None,
			},
		)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let activity = run.child_agent_activity.as_ref().expect("activity should render");
	let protocol_activity =
		run.protocol_activity.as_ref().expect("protocol fallback should render");
	let tracker_bucket =
		activity.buckets.iter().find(|bucket| bucket.name == "Tracker").expect("tracker bucket");

	assert_eq!(run.wait_reason.as_deref(), Some("tool_execution"));
	assert_eq!(protocol_activity.waiting_reason.as_deref(), Some("tool_execution"));
	assert_eq!(run.lifecycle_metrics.attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.captured_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.tool_call_count, 1);
	assert_eq!(run.lifecycle_metrics.phases.len(), 1);
	assert_eq!(run.lifecycle_metrics.phases[0].phase, "development");
	assert!(activity.current_elapsed_seconds.is_some_and(|elapsed| elapsed >= 90));
	assert!(
		tracker_bucket.wall_seconds >= 90,
		"current tool-call elapsed time should contribute to tracker bucket wall time"
	);
}

#[test]
fn operator_status_current_lane_lifecycle_reconstructs_all_issue_attempts() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[tracker::automation_active_label(TEST_SERVICE_ID).as_str()]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let development_activity = ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 600,
			event_count: 2,
			tool_call_count: 1,
			input_tokens: 100,
			output_tokens: 30,
			..state::ChildAgentActivityBucket::default()
		}],
		wall_seconds: 600,
		event_count: 2,
		tool_call_count: 1,
		input_tokens_cumulative: 100,
		output_tokens_cumulative: 30,
		..ChildAgentActivitySummary::default()
	};
	let review_activity = ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds: 300,
			event_count: 3,
			tool_call_count: 2,
			input_tokens: 200,
			output_tokens: 40,
			..state::ChildAgentActivityBucket::default()
		}],
		wall_seconds: 300,
		event_count: 3,
		tool_call_count: 2,
		input_tokens_cumulative: 200,
		output_tokens_cumulative: 40,
		..ChildAgentActivitySummary::default()
	};

	state_store
		.record_run_attempt("run-development", &issue.id, 1, "failed")
		.expect("development attempt should record");
	state_store
		.record_run_activity_summary("run-development", 1, Some(&development_activity), None)
		.expect("development activity should record");
	state_store
		.record_run_attempt("run-review", &issue.id, 2, "running")
		.expect("review attempt should record");
	state_store
		.record_run_activity_summary("run-review", 2, Some(&review_activity), None)
		.expect("review activity should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-review", "In Progress")
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
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			run_id: "run-review",
			attempt_number: 2,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 0)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(run.lifecycle_metrics.attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.captured_attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.missing_attempt_count, 0);
	assert_eq!(run.lifecycle_metrics.tool_call_count, 3);
	assert_eq!(run.lifecycle_metrics.input_tokens_cumulative, 300);
	assert_eq!(run.lifecycle_metrics.output_tokens_cumulative, 70);
	assert_eq!(run.lifecycle_metrics.phases.len(), 2);
	assert_eq!(run.lifecycle_metrics.phases[0].phase, "development");
	assert_eq!(run.lifecycle_metrics.phases[0].attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.phases[0].wall_seconds, 600);
	assert_eq!(run.lifecycle_metrics.phases[1].phase, "review");
	assert_eq!(run.lifecycle_metrics.phases[1].attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.phases[1].wall_seconds, 300);
}

fn sample_lifecycle_activity(
	wall_seconds: i64,
	event_count: i64,
	tool_call_count: i64,
	input_tokens: i64,
	output_tokens: i64,
) -> ChildAgentActivitySummary {
	ChildAgentActivitySummary {
		buckets: vec![state::ChildAgentActivityBucket {
			name: String::from("Model"),
			wall_seconds,
			event_count,
			tool_call_count,
			input_tokens,
			output_tokens,
			..state::ChildAgentActivityBucket::default()
		}],
		wall_seconds,
		event_count,
		tool_call_count,
		input_tokens_cumulative: input_tokens,
		output_tokens_cumulative: output_tokens,
		..ChildAgentActivitySummary::default()
	}
}

#[test]
fn operator_status_current_lane_lifecycle_recovers_from_local_evidence_after_restart() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[tracker::automation_active_label(TEST_SERVICE_ID).as_str()]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let development_activity = sample_lifecycle_activity(480, 4, 2, 600, 120);
	let review_activity = sample_lifecycle_activity(240, 3, 1, 300, 90);

	state_store
		.upsert_lease("pubfi", &issue.id, "run-review", "In Progress")
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
		.record_run_activity_summary("run-development", 1, Some(&development_activity), None)
		.expect("development activity should record");
	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-development",
			1,
			"issue_progress_checkpoint",
			serde_json::json!({ "source": "restart-recovery-test" }),
		)
		.expect("development private evidence should record");
	state_store
		.record_run_activity_summary("run-review", 2, Some(&review_activity), None)
		.expect("review activity should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			run_id: "run-review",
			attempt_number: 2,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-review",
			attempt_number: 2,
			thread_id: Some("thread-review"),
			turn_id: Some("turn-review"),
			event_count: 3,
			last_event_type: "model/response",
			child_agent_activity: Some(&review_activity),
			protocol_activity: None,
		},
	)
	.expect("worktree activity marker should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 0)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should recover");

	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(run.run_id, "run-review");
	assert_eq!(run.lifecycle_metrics.attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.recorded_attempt_count, 0);
	assert_eq!(run.lifecycle_metrics.recovered_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.current_snapshot_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.captured_attempt_count, 2);
	assert_eq!(run.lifecycle_metrics.tool_call_count, 3);
	assert_eq!(run.lifecycle_metrics.input_tokens_cumulative, 900);
	assert_eq!(run.lifecycle_metrics.output_tokens_cumulative, 210);
	assert_eq!(run.lifecycle_metrics.phases.len(), 2);
	assert_eq!(run.lifecycle_metrics.phases[0].phase, "development");
	assert_eq!(run.lifecycle_metrics.phases[0].recovered_attempt_count, 1);
	assert_eq!(run.lifecycle_metrics.phases[1].phase, "review");
	assert_eq!(run.lifecycle_metrics.phases[1].current_snapshot_attempt_count, 1);
	assert!(
		run.lifecycle_metrics
			.attempt_evidence
			.iter()
			.any(|attempt| attempt.run_id == "run-development"
				&& attempt.source == "recovered"
				&& attempt
					.evidence
					.iter()
					.any(|evidence| evidence == "private_execution_event:issue_progress_checkpoint"))
	);
	assert!(
		run.lifecycle_metrics
			.attempt_evidence
			.iter()
			.any(|attempt| attempt.run_id == "run-review"
				&& attempt.source == "current_snapshot"
				&& attempt
					.evidence
					.iter()
					.any(|evidence| evidence == "worktree_activity_marker"))
	);
}

#[test]
fn operator_status_snapshot_uses_structured_protocol_activity_summary() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("approval_or_user_input")),
		rate_limit_status: Some(String::from("primary")),
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("plan/update"),
				category: String::from("plan"),
				detail: Some(String::from("verify")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("item/tool/requestUserInput"),
				category: String::from("item"),
				detail: None,
			},
		],
	};

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
			event_count: 2,
			last_event_type: "item/tool/requestUserInput",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.wait_reason.as_deref(), Some("approval_or_user_input"));
	assert_eq!(run.protocol_activity.as_ref(), Some(&protocol_activity));
	assert!(rendered.contains("protocol_activity: turn=running; waiting=approval_or_user_input; rate_limit=primary; recent=item/tool/requestUserInput, plan/update:verify"));
}

#[test]
fn operator_status_snapshot_sanitizes_private_protocol_activity_details() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");
	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("running")),
		waiting_reason: Some(String::from("tool_execution")),
		rate_limit_status: None,
		recent_events: vec![
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker path=/srv/decodex/runtime")),
			},
			state::ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker (/srv/decodex/runtime)")),
			},
		],
	};

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
			event_count: 2,
			last_event_type: "configWarning",
			child_agent_activity: None,
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);
	let summary = run.protocol_activity.as_ref().expect("protocol summary should render");

	assert!(
		summary
			.recent_events
			.iter()
			.all(|event| event.detail.as_deref() == Some("redacted_sensitive_detail"))
	);
	assert!(rendered.contains("configWarning:redacted_sensitive_detail"));
	assert!(!rendered.contains("path=/srv"));
	assert!(!rendered.contains("(/srv"));
}

#[test]
fn operator_status_snapshot_ignores_marker_from_newer_attempt_for_stored_run() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("Todo", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "failed")
		.expect("stored run should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker_for_process(&worktree_path, "run-2", 2, process::id())
		.expect("newer attempt marker should write");
	state::write_run_retry_schedule(
		&worktree_path,
		"run-2",
		2,
		"failure",
		OffsetDateTime::now_utc().unix_timestamp() + 60,
	)
	.expect("retry schedule marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.recent_runs.first().expect("recent run should exist");

	assert_eq!(run.run_id, "run-1");
	assert_eq!(run.phase, "failed");
	assert_eq!(run.wait_reason, None);
	assert_eq!(run.process_id, None);
	assert_eq!(run.process_alive, None);
	assert_eq!(run.retry_kind, None);
	assert_eq!(run.next_retry_at, None);
}

#[test]
fn operator_status_snapshot_keeps_all_current_lanes_when_recent_runs_are_limited() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue = sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let second_issue = sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T04:17:17.133Z",
	);

	for (run_id, issue, branch_suffix) in
		[("run-1", &first_issue, "101"), ("run-2", &second_issue, "102")]
	{
		state_store
			.record_run_attempt(run_id, &issue.id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease("pubfi", &issue.id, run_id, "In Progress")
			.expect("lease should record");
		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				&format!("x/pubfi-pub-{branch_suffix}"),
				&config.worktree_root().join(&issue.identifier).display().to_string(),
			)
			.expect("worktree should record");
	}

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 1)
		.expect("snapshot should build");

	assert_eq!(snapshot.run_limit, 1);
	assert_eq!(snapshot.recent_runs.len(), 2);
	assert_eq!(snapshot.current_lanes.len(), 2);
	assert!(snapshot.current_lanes.iter().all(|run| run.run_lease));
}

#[test]
fn operator_status_snapshot_keeps_terminal_run_after_lane_cleanup() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Done",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);

	state_store.record_run_attempt("run-done", &issue.id, 1, "running").expect("run should record");
	state_store
		.upsert_lease("pubfi", &issue.id, "run-done", "In Progress")
		.expect("run lease should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&config.worktree_root().join(&issue.identifier).display().to_string(),
		)
		.expect("worktree should record");
	state_store.update_run_status("run-done", "succeeded").expect("terminal status should update");
	state_store.clear_lease(&issue.id).expect("terminal cleanup should clear run lease");
	state_store.clear_worktree(&issue.id).expect("terminal cleanup should clear worktree");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 25)
		.expect("snapshot should build");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot.recent_runs.len(), 1);
	assert_eq!(snapshot.recent_runs[0].run_id, "run-done");
	assert_eq!(snapshot.recent_runs[0].phase, "completed");
	assert!(!snapshot.recent_runs[0].run_lease);
	assert_eq!(snapshot.recent_runs[0].branch_name, None);
	assert_eq!(snapshot.recent_runs[0].worktree_path, None);
	assert_eq!(snapshot.history_lanes.len(), 1);
	assert_eq!(snapshot.history_lanes[0].latest_run.run_id, "run-done");
	assert!(rendered.contains("Run ledger shown: 1 issue lanes from 1 history attempts"));
	assert!(rendered.contains("run_id: run-done"));
}

#[test]
fn status_hydration_does_not_fabricate_run_leases_for_recovered_candidates() {
	let (_temp_dir, config, _workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("In Progress", &[]);
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	orchestrator::hydrate_status_snapshot_state(
		&config,
		&state_store,
		RecoveredRuntimeState { recoverable_issues: vec![issue.clone()] },
	)
	.expect("status hydration should succeed");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert!(
		snapshot.current_lanes.is_empty(),
		"recovered retry candidates should not appear as run leased runs"
	);
	assert!(
		snapshot.recent_runs.is_empty(),
		"status hydration should not persist synthetic recovered runs"
	);
}

#[test]
fn live_operator_status_snapshot_hydrates_current_lane_thread_and_event_metadata_from_marker() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = sample_issue("In Progress", &[active_label.as_str()]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_activity_marker(&worktree_path, "run-1", 1)
		.expect("activity marker should write");
	state::write_run_thread_marker(&worktree_path, "run-1", 1, "thread-1")
		.expect("thread marker should write");
	state::write_run_turn_marker(&worktree_path, "run-1", 1, "turn-1")
		.expect("turn marker should write");
	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1",
		1,
		Some("thread-1"),
		Some("turn-1"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("thread status should write");
	state::write_run_effective_runtime_marker(
		&worktree_path,
		"run-1",
		1,
		&EffectiveRuntimeMarker {
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			effective_model: "gpt-5.4",
			effective_model_provider: "openai",
			effective_cwd: "/tmp/worktree",
			effective_approval_policy: "never",
			effective_approvals_reviewer: "human",
			effective_sandbox_mode: "workspaceWrite",
		},
	)
	.expect("effective runtime should write");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
				event_count: 2,
				last_event_type: "turn/completed",
				child_agent_activity: None,
				protocol_activity: None,
			},
		)
	.expect("protocol summary should write");

	let recovered_state = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");

	orchestrator::hydrate_status_snapshot_state(&config, &state_store, recovered_state)
		.expect("status hydration should succeed");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(snapshot.current_lanes.len(), 1);
	assert_eq!(snapshot.current_lanes[0].thread_id.as_deref(), Some("thread-1"));
	assert_eq!(snapshot.current_lanes[0].turn_id.as_deref(), Some("turn-1"));
	assert_eq!(snapshot.current_lanes[0].thread_status.as_deref(), Some("active"));
	assert_eq!(
		snapshot.current_lanes[0].thread_active_flags,
		vec![String::from("waitingOnApproval")]
	);
	assert!(snapshot.current_lanes[0].interactive_requested);
	assert_eq!(snapshot.current_lanes[0].event_count, 2);
	assert_eq!(snapshot.current_lanes[0].last_event_type.as_deref(), Some("turn/completed"));
	assert_eq!(snapshot.current_lanes[0].effective_model.as_deref(), Some("gpt-5.4"));
	assert_eq!(snapshot.current_lanes[0].effective_model_provider.as_deref(), Some("openai"));
	assert_eq!(snapshot.current_lanes[0].effective_approval_policy.as_deref(), Some("never"));
	assert_eq!(snapshot.current_lanes[0].effective_sandbox_mode.as_deref(), Some("workspaceWrite"));
	assert!(snapshot.current_lanes[0].last_event_at.is_some());
}
