use crate::orchestrator::tests::operator::status::running_lanes::{
	self, Command, Duration, Instant, ProjectRegistration, ProtocolActivityMarker,
	ReviewHandoffMarker, StateStore, fs, orchestrator, process, state, thread,
};
#[test]
fn operator_status_snapshot_counts_stopped_active_process_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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

#[test]
fn operator_status_snapshot_treats_dead_leased_app_server_run_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.status, "running");
	assert_eq!(run.attempt_status, "running");
	assert!(run.run_lease);
	assert_eq!(run.queue_lease_state, "held");
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_stopped"));
	assert_eq!(run.thread_status.as_deref(), Some("active"));
	assert_eq!(run.liveness_state, "not_running");
	assert!(run.has_fresh_execution);
	assert!(!run.counts_as_running);
	assert!(run.needs_attention);
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[cfg(unix)]
#[test]
fn operator_status_snapshot_counts_zombie_active_process_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	running_lanes::rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

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
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	running_lanes::rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

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
	assert!(
		run.lane_control_conditions.iter().any(|condition| condition == "host_boot_id_mismatch")
	);
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
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	running_lanes::rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

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
fn operator_status_snapshot_post_review_lane_owns_orphaned_live_thread_worktree() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("In Review", &[]);
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

	let mut snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	snapshot.post_review_lanes = vec![orchestrator::OperatorPostReviewLaneStatus {
		project_id: String::from("pubfi"),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: String::from("In Review"),
		branch_name: String::from("x/pubfi-pub-101"),
		worktree_path: String::from(".worktrees/PUB-101"),
		classification: String::from("wait_for_review"),
		reason: String::from("non_github_review_waiting_gates"),
		pr_url: Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/101")),
		pr_head_sha: Some(String::from("1111111111111111111111111111111111111111")),
		pr_state: Some(String::from("OPEN")),
		review_decision: None,
		mergeable: Some(String::from("MERGEABLE")),
		check_state: Some(String::from("PENDING")),
		unresolved_review_threads: Some(0),
		shadowed_by_current_lane: false,
		readback_warning: None,
		readback_root_cause: None,
		loop_status: None,
	}];

	orchestrator::hydrate_post_review_lane_current_lane_shadowing(&mut snapshot);
	orchestrator::refresh_worktree_ownership(&mut snapshot, None);
	orchestrator::refresh_operator_project_summary(&mut snapshot, None);

	let project = snapshot.projects.first().expect("project summary should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(snapshot.recent_runs[0].ownership_state, "orphaned_live_thread");
	assert_eq!(snapshot.worktrees[0].ownership, "post_review_lane");
	assert_eq!(
		snapshot.worktrees[0].ownership_reason,
		"Review & Landing owns this worktree as `wait_for_review`."
	);
	assert_eq!(snapshot.worktrees[0].recovery_next_action, None);
	assert!(!snapshot.worktrees[0].provenance.audit_required);
	assert_eq!(project.post_review_lane_count, 1);
	assert_eq!(project.retained_worktree_count, 1);
	assert!(rendered.contains("role: post_review_lane"));
	assert!(rendered.contains("recovery_next_action: none"));
	assert!(!rendered.contains("role: orphaned_live_thread"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn operator_status_snapshot_counts_reused_pid_as_attention_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	running_lanes::rewrite_run_activity_marker_process_start_identity(
		&worktree_path,
		"previous-process-start",
	);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should remain visible");
	let project = snapshot.projects.first().expect("project summary should exist");

	assert_eq!(run.phase, "executing");
	assert_eq!(run.process_id, Some(process::id()));
	assert_eq!(run.process_alive, Some(false));
	assert_eq!(run.execution_liveness, "process_identity_mismatch");
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_start_identity_mismatch"));
	assert_eq!(project.current_lane_count, 1);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.attention_count, 1);
}

#[test]
fn operator_status_snapshot_keeps_unleased_live_process_visible_but_not_running() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	assert_eq!(run.lane_control_next_action, "inspect_or_interrupt_orphaned_live_thread");
	assert!(run.lane_control_conditions.iter().any(|condition| condition == "run_lease_missing"));
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
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	assert!(
		run.lane_control_conditions
			.iter()
			.any(|condition| condition == "terminal_attempt_has_live_evidence")
	);
	assert_eq!(run.process_alive, Some(true));
	assert_eq!(run.process_liveness_reason.as_deref(), Some("process_alive"));
	assert_eq!(project.current_lane_count, 0);
	assert_eq!(project.running_lane_count, 0);
	assert_eq!(project.retained_worktree_count, 1);
	assert_eq!(snapshot.worktrees[0].ownership, "orphaned_live_thread");
}

#[test]
fn operator_status_snapshot_excludes_terminal_thread_archive_from_running_lanes() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);

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
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	running_lanes::rewrite_run_activity_marker_host_boot_id(&worktree_path, "previous-boot");

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
	assert!(
		run.lane_control_conditions.iter().any(|condition| condition == "host_boot_id_mismatch")
	);
}

#[test]
fn operator_status_snapshot_keeps_succeeded_status_live_process_in_recent_orphan_bucket() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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

	running_lanes::assert_terminal_pending_status_projection(&snapshot);
	running_lanes::assert_terminal_pending_lane_inspect(&state_store);
	running_lanes::assert_terminal_pending_interrupt_rejects_force(&state_store);

	if matches!(child.try_wait(), Ok(None)) {
		child.kill().expect("sleep child should be killable");
	}

	child.wait().expect("sleep child should reap");
}

#[test]
fn operator_status_projects_terminal_finalized_handoff_missing_lifecycle_marker() {
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
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"review_completion_intent",
			serde_json::json!({
				"path": "review_handoff",
				"mode": "handoff",
				"branch": "x/pubfi-pub-101",
				"worktree_path": worktree_path.display().to_string(),
				"pr_url": "https://github.com/hack-ink/decodex/pull/101",
				"pr_base_ref": "main",
				"pr_head_ref": "x/pubfi-pub-101",
				"pr_head_oid": "08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
				"summary": "Ready for review."
			}),
		)
		.expect("handoff intent should record");
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

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot
		.recent_runs
		.iter()
		.find(|run| run.run_id == "run-1")
		.expect("terminal-pending run should remain inspectable in recent runs");

	assert_eq!(run.status, "review_handoff_pending");
	assert_eq!(run.phase, "terminal_pending");
	assert_eq!(
		run.wait_reason.as_deref(),
		Some("review_handoff_writeback_missing_lifecycle_marker")
	);
	assert_eq!(run.current_operation, state::RUN_OPERATION_REVIEW_WRITEBACK);
}

#[test]
fn operator_status_projects_terminal_finalized_repair_missing_lifecycle_marker() {
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
	let pr_head_oid = "08a20f7dfb9526e7421a5f095b1c6adec84e52d6";

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
				"pr_head_oid": pr_head_oid,
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
		Some("review_repair_writeback_missing_lifecycle_marker")
	);
	assert_eq!(run.current_operation, state::RUN_OPERATION_REVIEW_WRITEBACK);
}

#[test]
fn operator_status_projects_terminal_finalized_repair_stale_lifecycle_marker() {
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
		.upsert_review_handoff_marker(
			"pubfi",
			&issue.id,
			&ReviewHandoffMarker::new(
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
	assert_eq!(run.wait_reason.as_deref(), Some("review_repair_writeback_stale_lifecycle_marker"));
	assert_eq!(run.current_operation, state::RUN_OPERATION_REVIEW_WRITEBACK);
}
