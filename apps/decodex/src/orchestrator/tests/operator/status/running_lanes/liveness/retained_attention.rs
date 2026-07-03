use crate::orchestrator::tests::operator::status::running_lanes::{
	self, StateStore, fs, orchestrator, process, state,
};

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
