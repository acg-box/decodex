use super::*;

#[test]
fn stale_active_diagnose_classifies_tracker_present_active_without_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store.update_run_thread("run-1626", "thread-stale").expect("thread should record");
	store.update_run_turn("run-1626", "turn-stale").expect("turn should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert_eq!(
		diagnostic.reason,
		"tracker_issue_has_stale_active_label_without_live_or_retained_progress"
	);
	assert!(diagnostic.active_label_present);
	assert!(diagnostic.queue_label_present);
	assert!(!diagnostic.run_lease);
	assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-1626"));
	assert!(diagnostic.blockers.is_empty(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("tracker_issue_present")));
	assert!(diagnostic.evidence.contains(&String::from("run_lease_missing")));
	assert!(diagnostic.evidence.contains(&String::from("private_evidence_missing")));
	assert!(diagnostic.evidence.contains(&String::from("stale_thread_reference_present")));
	assert!(diagnostic.next_action.contains("recover stale-active release PUB-1626 --dry-run"));
}

#[test]
fn stale_active_diagnose_blocks_shared_claim_lock_file() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let owner_store = StateStore::open_in_memory().expect("owner store should open");
	let store = StateStore::open_in_memory().expect("reader store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");
	owner_store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("owner store should configure dispatch root");
	assert!(
		owner_store
			.try_acquire_lease("pubfi", &issue.id, "run-live", "In Progress")
			.expect("owner should acquire shared claim")
	);
	store
		.observe_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("reader store should observe dispatch root");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.active_shared_claim);
	assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_run_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	store
		.upsert_lease("pubfi", &issue.identifier, "run-identifier", "In Progress")
		.expect("identifier-keyed lease should record");
	store
		.record_run_attempt("run-identifier", &issue.identifier, 1, "running")
		.expect("identifier-keyed run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.identifier,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("identifier-keyed worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-identifier"));
	assert!(diagnostic.run_lease);
	assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_private_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_private_execution_event(
			"pubfi",
			&issue.identifier,
			"run-identifier",
			1,
			"source_progress",
			serde_json::json!({"phase": "implementation"}),
		)
		.expect("identifier-keyed private progress should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_worktree_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("identifier-worktree");

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	fs::create_dir_all(&worktree_path).expect("identifier worktree should create");
	fs::write(worktree_path.join("source.rs"), "fn progress() {}\n")
		.expect("ordinary worktree file should write");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.identifier,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("identifier-keyed worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "non_git_files_present");
	assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_active_thread_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	fs::create_dir_all(&worktree_path).expect("worktree path should create");
	state::write_run_thread_status_marker(
		&worktree_path,
		"run-1626",
		1,
		Some("thread-1626"),
		Some("turn-1626"),
		"active",
		&[String::from("waitingOnApproval")],
	)
	.expect("active thread marker should write");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("activity_marker_thread_active")));
	assert!(!diagnostic.recoverable());
}

fn dead_orphan_activity_summaries() -> (ChildAgentActivitySummary, ProtocolActivitySummary) {
	(
		ChildAgentActivitySummary {
			event_count: 531,
			current_bucket: Some(String::from("Model")),
			..ChildAgentActivitySummary::default()
		},
		ProtocolActivitySummary {
			turn_status: Some(String::from("running")),
			waiting_reason: Some(String::from("model_execution")),
			..ProtocolActivitySummary::default()
		},
	)
}

fn seed_dead_orphan_runtime_telemetry(
	store: &StateStore,
	issue: &TrackerIssue,
	worktree_path: &Path,
) {
	let control_channel_path = worktree_path.join(".decodex-run-control/run-1626-1.channel");
	let (child_activity, protocol_activity) = dead_orphan_activity_summaries();

	init_clean_git_repo_with_remote_default(worktree_path, "x/pubfi-pub-1626");
	state::write_run_activity_marker_for_process(worktree_path, "run-1626", 1, u32::MAX)
		.expect("stale process marker should write");
	state::write_run_protocol_activity_marker(
		worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1626",
			attempt_number: 1,
			thread_id: Some("thread-stale"),
			turn_id: Some("turn-stale"),
			event_count: 531,
			last_event_type: "item/started",
			child_agent_activity: Some(&child_activity),
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("stale protocol marker should write");
	state::write_run_thread_status_marker(
		worktree_path,
		"run-1626",
		1,
		Some("thread-stale"),
		Some("turn-stale"),
		"active",
		&[],
	)
	.expect("stale thread marker should write");
	fs::create_dir_all(control_channel_path.parent().expect("channel parent"))
		.expect("control directory should create");
	fs::write(
		&control_channel_path,
		"schema=decodex.run_control_channel/v1\nrun_id=run-1626\nattempt_number=1\n",
	)
	.expect("control channel file should write");
	store.record_run_attempt("run-1626", &issue.id, 1, "running").expect("run attempt");
	store
		.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
		.expect("temporary lease should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.publish_run_control_channel_for_active_attempt(
			"run-1626",
			1,
			&control_channel_path,
			"local_file",
		)
		.expect("control channel should publish");
	store.clear_lease(&issue.id).expect("stale lane lease should clear");
	store
		.append_event("run-1626", 1, "item/started", r#"{"kind":"model"}"#)
		.expect("protocol event should record");
	store
		.record_run_activity_summary("run-1626", 1, Some(&child_activity), Some(&protocol_activity))
		.expect("activity summary should record");
	append_dead_orphan_private_telemetry(store, &issue.id);
}

fn seed_dead_orphan_runtime_telemetry_without_control_channel(
	store: &StateStore,
	issue: &TrackerIssue,
	worktree_path: &Path,
) {
	let (child_activity, protocol_activity) = dead_orphan_activity_summaries();

	state::write_run_activity_marker_for_process(worktree_path, "run-1626", 1, u32::MAX)
		.expect("stale process marker should write");
	state::write_run_protocol_activity_marker(
		worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1626",
			attempt_number: 1,
			thread_id: Some("thread-stale"),
			turn_id: Some("turn-stale"),
			event_count: 531,
			last_event_type: "item/started",
			child_agent_activity: Some(&child_activity),
			protocol_activity: Some(&protocol_activity),
		},
	)
	.expect("stale protocol marker should write");
	state::write_run_thread_status_marker(
		worktree_path,
		"run-1626",
		1,
		Some("thread-stale"),
		Some("turn-stale"),
		"active",
		&[],
	)
	.expect("stale thread marker should write");
	store
		.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
		.expect("temporary lease should record");
	store.record_run_attempt("run-1626", &issue.id, 1, "running").expect("run attempt");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	store.clear_lease(&issue.id).expect("stale lane lease should clear");
	store
		.append_event("run-1626", 1, "item/started", r#"{"kind":"model"}"#)
		.expect("protocol event should record");
	store
		.record_run_activity_summary("run-1626", 1, Some(&child_activity), Some(&protocol_activity))
		.expect("activity summary should record");
	append_dead_orphan_private_telemetry_without_control_channel_marker(store, &issue.id);
}

fn append_dead_orphan_private_telemetry(store: &StateStore, issue_id: &str) {
	append_dead_orphan_private_telemetry_events(store, issue_id, true);
}

fn append_dead_orphan_private_telemetry_without_control_channel_marker(
	store: &StateStore,
	issue_id: &str,
) {
	append_dead_orphan_private_telemetry_events(store, issue_id, false);
}

fn append_dead_orphan_private_telemetry_events(
	store: &StateStore,
	issue_id: &str,
	include_control_channel_marker: bool,
) {
	let mut events = vec![
		(
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
			}),
		),
		(
			"progress_checkpoint",
			serde_json::json!({
				"phase": "probing",
				"pr_url": null,
				"verification": [],
				"head_sha": "1111111111111111111111111111111111111111",
			}),
		),
		(
			"control_action",
			serde_json::json!({
				"action": "interrupt",
				"reason": "run_lease_missing",
			}),
		),
		(
			"lane_control/interrupt",
			serde_json::json!({
				"classification": "hard_interrupt_fallback",
				"processAliveAfter": false,
				"signals": [],
				"status": "sent",
			}),
		),
	];

	if include_control_channel_marker {
		events.insert(
			0,
			(
				"control_channel_published",
				serde_json::json!({
					"schema": "decodex.run_control_channel/v1",
					"status": "active",
				}),
			),
		);
	}

	for (event_type, payload) in events {
		store
			.append_private_execution_event("pubfi", issue_id, "run-1626", 1, event_type, payload)
			.expect("private stale telemetry should record");
	}
}

fn append_app_server_no_progress_failure_evidence(store: &StateStore, issue_id: &str) {
	for (event_type, payload) in [
		(
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"checkpoint_attempt_number": 1,
				"checkpoint_run_id": "run-1626",
				"consecutive_count": 1,
				"details": serde_json::json!({
					"branch_delta_present": false,
					"effective_delta_present": false,
					"reason": "no_effective_diff",
					"source_error_class": "app_server_turn_failed",
				})
				.to_string(),
				"fingerprint": "empty:empty",
				"reason": "no_effective_diff",
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"source_error_class": "app_server_turn_failed",
				"threshold": 3,
			}),
		),
		(
			"harness_outcome",
			serde_json::json!({
				"authority_boundary": {
					"dispositions": [],
					"failed_check_count": 0,
					"improvement_signal_count": 0,
				},
				"contracts": [],
				"execution_programs": [],
				"linear_projection": {
					"event_types": ["run_started"],
					"final_error_class": null,
					"final_event_type": "run_started",
					"final_terminal_path": null,
				},
				"manual_attention": null,
				"phase_goal_outcomes": [{
					"event_type": "phase_goal_set",
					"phase": "implement_to_validation_ready",
					"status": "active",
				}],
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"record_version": 1,
				"repair": {
					"attempt_number": 1,
					"repair_attempt_observed": false,
					"repair_phase_events": 0,
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"attempt_number": 1,
					"issue_id": issue_id,
					"issue_identifier": "PUB-1626",
					"outcome": "retryable_failure",
					"project_id": "pubfi",
					"run_id": "run-1626",
					"source_intents": [],
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		),
	] {
		store
			.append_private_execution_event("pubfi", issue_id, "run-1626", 1, event_type, payload)
			.expect("private no-progress failure evidence should record");
	}
}

fn append_no_diff_guardrail_event(
	store: &StateStore,
	issue_id: &str,
	branch_delta_present: bool,
	effective_delta_present: bool,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"loop_guardrail_checkpoint",
			serde_json::json!({
				"details": serde_json::json!({
					"branch_delta_present": branch_delta_present,
					"effective_delta_present": effective_delta_present,
				})
				.to_string(),
				"reason": "no_effective_diff",
				"schema": "decodex.loop_guardrail_checkpoint/1",
				"source_error_class": "app_server_turn_failed",
			}),
		)
		.expect("private guardrail evidence should record");
}

fn append_harness_outcome_with_pr_progress(store: &StateStore, issue_id: &str) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": ["https://github.com/hack-ink/pubfi/pull/1631"],
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		)
		.expect("private harness progress evidence should record");
}

fn append_harness_outcome_with_review_progress(store: &StateStore, issue_id: &str) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"contracts": [],
				"execution_programs": [],
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"review": {
					"accepted_finding_count": 1,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": [],
					"failure_count": 0,
					"result": "not_recorded",
				},
			}),
		)
		.expect("private harness review progress evidence should record");
}

fn append_harness_outcome_with_validation_progress(store: &StateStore, issue_id: &str) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"harness_outcome",
			serde_json::json!({
				"contracts": [],
				"execution_programs": [],
				"manual_attention": null,
				"pr_lifecycle": {
					"outcome": "retryable_failure",
					"pr_urls": [],
				},
				"review": {
					"accepted_finding_count": 0,
					"nonclean_rounds": 0,
					"rejected_finding_count": 0,
					"statuses": [],
				},
				"schema": "decodex.harness_outcome/1",
				"source": {
					"outcome": "retryable_failure",
				},
				"validation": {
					"failure_classes": ["repo_gate_verify_failed"],
					"failure_count": 1,
					"result": "failed",
				},
			}),
		)
		.expect("private harness validation progress evidence should record");
}

fn append_phase_goal_recovery_event(
	store: &StateStore,
	issue_id: &str,
	phase: &str,
	source_error_class: &str,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			"run-1626",
			1,
			"phase_goal_recovery",
			serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": phase,
					"signal": "phase_goal_recovered",
					"payload": {
						"nextPhase": "handoff_evidence",
					"sourceErrorClass": source_error_class,
					"sourceErrorMessage": "runtime failure",
				},
			}),
		)
		.expect("private phase goal recovery evidence should record");
}

fn append_stale_active_release_audit(store: &StateStore, issue_id: &str) {
	append_stale_active_release_audit_for_run(store, issue_id, "run-1626", 1);
}

fn append_stale_active_release_audit_for_run(
	store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			run_id,
			attempt_number,
			STALE_ACTIVE_RELEASE_EVENT,
			serde_json::json!({
				"schema": STALE_ACTIVE_RECOVERY_SCHEMA,
				"event": STALE_ACTIVE_RELEASE_EVENT,
				"phase": "local_cleanup_complete_before_active_label_release",
			}),
		)
		.expect("stale active release audit should record");
}

#[test]
fn stale_active_diagnose_allows_dead_orphan_thread_runtime_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("process_not_alive")));
	assert!(diagnostic.evidence.contains(&String::from("stale_active_control_channel_present")));
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("only_stale_active_or_failed_control_evidence_present"))
	);
}

#[test]
fn stale_active_diagnose_allows_app_server_no_progress_failure_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_app_server_no_progress_failure_evidence(&store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(
		diagnostic
			.evidence
			.contains(&String::from("only_stale_active_or_failed_control_evidence_present"))
	);
}

#[test]
fn stale_active_diagnose_blocks_harness_outcome_with_pr_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_harness_outcome_with_pr_progress(&store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_harness_outcome_with_review_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_harness_outcome_with_review_progress(&store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_harness_outcome_with_validation_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_harness_outcome_with_validation_progress(&store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_no_diff_guardrail_with_delta() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_no_diff_guardrail_event(&store, &issue.id, true, false);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_allows_app_server_phase_goal_recovery_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_phase_goal_recovery_event(
		&store,
		&issue.id,
		"implement_to_validation_ready",
		"app_server_dynamic_tool_protocol_failure",
	);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
}

#[test]
fn stale_active_diagnose_blocks_repo_gate_phase_goal_recovery_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_phase_goal_recovery_event(
		&store,
		&issue.id,
		"implement_to_validation_ready",
		"repo_gate_verify_failed",
	);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_repair_phase_goal_recovery_telemetry() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let queue_label = tracker::automation_queue_label("pubfi");
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);
	append_phase_goal_recovery_event(
		&store,
		&issue.id,
		"repair_accepted_review_findings",
		"app_server_dynamic_tool_protocol_failure",
	);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_clean_worktree_with_unmerged_commits() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_git_repo(&worktree_path);
	run_git(&worktree_path, &["checkout", "-B", "main"]);
	commit_test_file(&worktree_path, "README.md", "base\n", "base");
	run_git(&worktree_path, &["checkout", "-b", "x/pubfi-pub-1626"]);
	commit_test_file(&worktree_path, "source.rs", "fn retained_progress() {}\n", "progress");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "unmerged_commits_present");
	assert!(diagnostic.blockers.contains(&String::from("worktree_unmerged_commits_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_clean_git_worktree_without_default_branch() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_git_repo(&worktree_path);
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "default_branch_unavailable");
	assert!(diagnostic.blockers.contains(&String::from("worktree_default_branch_unavailable")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_release_allows_reentry_after_local_cleanup_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("In Progress", &[active_label.clone(), queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
	context
		.state_store
		.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
		.expect("run should terminalize");
	context
		.state_store
		.retire_run_control_channel_for_attempt("run-1626", 1, RUN_CONTROL_CHANNEL_STATUS_FAILED)
		.expect("control channel should retire");
	fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
	context
		.state_store
		.clear_worktree_mapping(&issue.id)
		.expect("issue-id worktree mapping should clear");
	append_stale_active_release_audit(&context.state_store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete")));
	super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("reentry release should remove active label");
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[(issue.id.clone(), String::from("state-todo"))]
	);
}

#[test]
fn stale_active_release_reentry_blocks_active_status_after_local_cleanup_audit() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
	context
		.state_store
		.update_run_status("run-1626", "running")
		.expect("run should carry active status");
	context
		.state_store
		.retire_run_control_channel_for_attempt("run-1626", 1, RUN_CONTROL_CHANNEL_STATUS_FAILED)
		.expect("control channel should retire");
	fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
	context
		.state_store
		.clear_worktree_mapping(&issue.id)
		.expect("issue-id worktree mapping should clear");
	append_stale_active_release_audit(&context.state_store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, "stale_active_recovery_blocked");
	assert!(!diagnostic.recoverable());
	assert!(diagnostic.blockers.contains(&String::from("protocol_event_evidence_present")));
	assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
	assert!(diagnostic.blockers.contains(&String::from("protocol_activity_present")));
}

#[test]
fn stale_active_release_reentry_terminal_guards_terminal_looking_audited_run() {
	for status in ["failed", "interrupted"] {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue =
			sample_issue_with_labels("In Progress", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
		context
			.state_store
			.update_run_status("run-1626", status)
			.expect("run should carry terminal-looking app-server status");
		context
			.state_store
			.retire_run_control_channel_for_attempt(
				"run-1626",
				1,
				RUN_CONTROL_CHANNEL_STATUS_FAILED,
			)
			.expect("control channel should retire");
		fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
		context
			.state_store
			.clear_worktree_mapping(&issue.id)
			.expect("issue-id worktree mapping should clear");
		append_stale_active_release_audit(&context.state_store, &issue.id);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "{status} blockers: {:?}", diagnostic.blockers);
		assert_eq!(diagnostic.latest_attempt_status.as_deref(), Some(status));
		assert!(diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete")));

		super::super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("reentry release should terminal-guard and remove active label");

		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
		assert_eq!(
			tracker.state_updates.borrow().as_slice(),
			&[(issue.id.clone(), String::from("state-todo"))]
		);
	}
}

#[test]
fn stale_active_release_allows_reentry_after_local_cleanup_without_control_channel() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("In Progress", &[active_label.clone(), queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
	seed_dead_orphan_runtime_telemetry_without_control_channel(
		&context.state_store,
		&issue,
		&worktree_path,
	);
	context
		.state_store
		.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
		.expect("run should terminalize");
	fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
	context
		.state_store
		.clear_worktree_mapping(&issue.id)
		.expect("issue-id worktree mapping should clear");
	append_stale_active_release_audit(&context.state_store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));
	assert!(diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete")));

	super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("reentry release should remove active label");
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
}

#[test]
fn stale_active_release_reentry_without_control_channel_blocks_private_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
	seed_dead_orphan_runtime_telemetry_without_control_channel(
		&context.state_store,
		&issue,
		&worktree_path,
	);
	context
		.state_store
		.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
		.expect("run should terminalize");
	fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
	context
		.state_store
		.clear_worktree_mapping(&issue.id)
		.expect("issue-id worktree mapping should clear");
	append_stale_active_release_audit(&context.state_store, &issue.id);
	append_harness_outcome_with_pr_progress(&context.state_store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_release_reentry_restores_startable_state_after_active_label_release() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("In Progress", &[queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
	context
		.state_store
		.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
		.expect("run should terminalize");
	context
		.state_store
		.retire_run_control_channel_for_attempt("run-1626", 1, RUN_CONTROL_CHANNEL_STATUS_FAILED)
		.expect("control channel should retire");
	fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
	context
		.state_store
		.clear_worktree_mapping(&issue.id)
		.expect("issue-id worktree mapping should clear");
	append_stale_active_release_audit(&context.state_store, &issue.id);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION);
	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(
		diagnostic.evidence.contains(&String::from("stale_active_startable_state_restore_pending"))
	);
	super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("state-restore reentry should complete");
	assert!(tracker.label_removals.borrow().is_empty());
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[(issue.id.clone(), String::from("state-todo"))]
	);
}

#[test]
fn stale_active_release_reentry_rejects_release_audit_from_other_run() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	seed_dead_orphan_runtime_telemetry(&context.state_store, &issue, &worktree_path);
	context
		.state_store
		.update_run_status("run-1626", GHOST_LANE_TERMINAL_STATUS)
		.expect("run should terminalize");
	context
		.state_store
		.retire_run_control_channel_for_attempt("run-1626", 1, RUN_CONTROL_CHANNEL_STATUS_FAILED)
		.expect("control channel should retire");
	fs::remove_dir_all(&worktree_path).expect("worktree should be removed");
	context
		.state_store
		.clear_worktree_mapping(&issue.id)
		.expect("issue-id worktree mapping should clear");
	append_stale_active_release_audit_for_run(&context.state_store, &issue.id, "run-older", 1);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(!diagnostic.recoverable());
	assert!(!diagnostic.evidence.contains(&String::from("stale_active_local_cleanup_complete")));
	assert!(diagnostic.blockers.contains(&String::from("protocol_activity_present")));
}

#[test]
fn stale_active_diagnose_blocks_private_progress_from_older_attempt() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-old", &issue.id, 1, "running")
		.expect("old run attempt should record");
	store
		.record_run_attempt("run-new", &issue.id, 2, "running")
		.expect("new run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-old",
			1,
			"source_progress",
			serde_json::json!({"phase": "implementation"}),
		)
		.expect("private progress should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-new"));
	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_protocol_event_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.append_event("run-1626", 1, "turn/item", r#"{"kind":"progress"}"#)
		.expect("protocol event should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("protocol_event_evidence_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_marker_protocol_activity_evidence() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1626",
			attempt_number: 1,
			thread_id: Some("thread-stale"),
			turn_id: Some("turn-stale"),
			event_count: 1,
			last_event_type: "turn/completed",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol marker should write");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("protocol_event_marker_present")));
	assert!(
		diagnostic.blockers.contains(&String::from("activity_marker_protocol_activity_present"))
	);
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_untracked_worktree_progress() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_git_repo(&worktree_path);
	fs::write(worktree_path.join("new_source.rs"), "fn retained_progress() {}\n")
		.expect("untracked source should write");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_non_git_retained_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	fs::create_dir_all(&worktree_path).expect("retained path should create");
	fs::write(worktree_path.join("retained.txt"), "retained work\n")
		.expect("retained file should write");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "non_git_files_present");
	assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_child_agent_activity_summary() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.record_run_activity_summary("run-1626", 1, Some(&activity), None)
		.expect("child activity should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_when_worktree_status_unknown() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let worktree_path = temp_dir.path().join("PUB-1626");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	fs::create_dir_all(&worktree_path).expect("worktree path should create");
	fs::write(worktree_path.join(".git"), "gitdir: /does/not/exist\n")
		.expect("invalid gitdir should write");
	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.worktree_state, "tracked_changes_unknown");
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_needs_attention_label() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let needs_attention_label = String::from("decodex:needs-attention");
	let mut issue = sample_issue_with_labels("Todo", &[active_label, needs_attention_label]);

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("needs_attention_label_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_review_policy_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: &issue.id,
			run_id: "run-1626",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("review checkpoint should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_review_policy_checkpoint() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id: "PUB-1626",
			run_id: "run-1626",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("identifier-keyed review checkpoint should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_pr_lineage() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "PUB-1626",
			issue_identifier: "PUB-1626",
			run_id: "run-1626",
			attempt_number: 1,
		},
		"review_handoff",
		String::from("2026-06-28T00:00:00Z"),
		"review_handoff",
	);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	event.branch = Some(String::from("x/pubfi-pub-1626"));
	event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
	event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
	event.pr_base_ref = Some(String::from("main"));
	event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
	event.validation_result = Some(String::from("passed"));
	event.summary = Some(String::from("Recorded review handoff lineage."));
	event.terminal_path = Some(String::from("review_handoff"));
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store.record_linear_execution_event(&event).expect("linear event should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_tracker_comment_pr_lineage() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: "pubfi",
			issue_id: "linear-issue-1626",
			issue_identifier: "PUB-1626",
			run_id: "run-1626",
			attempt_number: 1,
		},
		"review_handoff",
		String::from("2026-06-28T00:00:00Z"),
		"review_handoff",
	);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	event.branch = Some(String::from("x/pubfi-pub-1626"));
	event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
	event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
	event.pr_base_ref = Some(String::from("main"));
	event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
	event.validation_result = Some(String::from("passed"));
	event.summary = Some(String::from("Recorded review handoff lineage."));
	event.terminal_path = Some(String::from("review_handoff"));

	let comment = TrackerComment {
		body: records::append_structured_comment_record(
			&records::render_linear_execution_event_comment_body(&event, None),
			&event,
		)
		.expect("structured comment should serialize"),
		created_at: String::from("2026-06-28T00:00:00Z"),
	};

	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]).with_comments(vec![comment]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_identifier_keyed_review_lifecycle() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let marker = ReviewHandoffMarker::new(
		"run-1626",
		1,
		"x/pubfi-pub-1626",
		"https://github.com/hack-ink/decodex/pull/1626",
		"main",
		"x/pubfi-pub-1626",
		"2222222222222222222222222222222222222222",
	);

	issue.id = String::from("linear-issue-1626");
	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&temp_dir.path().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");
	store
		.upsert_review_handoff_marker("pubfi", "PUB-1626", &marker)
		.expect("identifier-keyed review lifecycle should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_diagnose_blocks_unreadable_activity_marker() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);
	let worktree_path = temp_dir.path().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	fs::create_dir_all(worktree_path.join(state::RUN_ACTIVITY_MARKER_FILE))
		.expect("directory marker should create");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
	assert!(
		diagnostic.evidence.iter().any(|entry| entry.starts_with("worktree_status_error:")),
		"diagnostic should include marker read error evidence: {:?}",
		diagnostic.evidence
	);
	assert!(!diagnostic.recoverable());
}

#[test]
fn stale_active_release_removes_active_label_and_terminalizes_stale_run() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&context.config.worktree_root().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("stale active release should apply");

	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");
	let events = context
		.state_store
		.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
		.expect("private events should read");

	assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
	assert!(events.iter().any(|event| {
		event.event_type() == STALE_ACTIVE_RELEASE_EVENT
			&& event.payload()["schema"] == super::super::STALE_ACTIVE_RECOVERY_SCHEMA
			&& event.payload()["active_label_release"] == "pending_final_mutation"
			&& event.payload()["phase"] == "local_cleanup_complete_before_active_label_release"
	}));
}

#[test]
fn stale_active_release_allows_final_reentry_when_control_channel_was_never_published() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_git_repo(context.config.repo_root());
	run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
	commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
	run_git(context.config.repo_root(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
	run_git(
		context.config.repo_root(),
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
	run_git(
		context.config.repo_root(),
		&[
			"worktree",
			"add",
			"-b",
			"x/pubfi-pub-1626",
			worktree_path.to_str().expect("worktree path should be utf-8"),
			"main",
		],
	);
	seed_dead_orphan_runtime_telemetry_without_control_channel(
		&context.state_store,
		&issue,
		&worktree_path,
	);

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
	assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));

	super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("stale active release should treat missing control channel as inactive reentry");

	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
}

#[test]
fn stale_active_release_terminal_guards_terminal_looking_run_before_final_safety_check() {
	for status in ["failed", "interrupted"] {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(context.config.repo_root());
		run_git(context.config.repo_root(), &["checkout", "-B", "main"]);
		commit_test_file(context.config.repo_root(), "README.md", "base\n", "base");
		run_git(context.config.repo_root(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
		run_git(
			context.config.repo_root(),
			&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
		);
		run_git(
			context.config.repo_root(),
			&[
				"worktree",
				"add",
				"-b",
				"x/pubfi-pub-1626",
				worktree_path.to_str().expect("worktree path should be utf-8"),
				"main",
			],
		);
		seed_dead_orphan_runtime_telemetry_without_control_channel(
			&context.state_store,
			&issue,
			&worktree_path,
		);
		context
			.state_store
			.update_run_status("run-1626", status)
			.expect("run should carry terminal-looking app-server status");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable(), "{status} blockers: {:?}", diagnostic.blockers);
		assert_eq!(diagnostic.latest_attempt_status.as_deref(), Some(status));

		super::super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("terminal-looking stale-active run should release after terminal guard");

		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}
}

#[test]
fn stale_active_release_removes_run_control_marker_only_directory() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");
	let control_dir = worktree_path.join(state::RUN_CONTROL_CHANNEL_DIR);

	issue.identifier = String::from("PUB-1626");
	fs::create_dir_all(&control_dir).expect("run-control marker directory should create");
	fs::write(control_dir.join("run-1626-1.channel"), "channel\n")
		.expect("run-control marker should write");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect("stale active release should apply");

	assert!(!worktree_path.exists(), "marker-only directory should be removed");
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
}

#[test]
fn stale_active_release_keeps_active_label_gate_when_tracker_label_removal_fails() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&context.config.worktree_root().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()])
		.remove_error("Linear label removal failed");
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	let error = super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("tracker removal failure should abort release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");
	let events = context
		.state_store
		.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
		.expect("private events should read");
	let mapping =
		context.state_store.worktree_for_issue(&issue.id).expect("worktree mapping should read");

	assert!(error.to_string().contains("Linear label removal failed"));
	assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
	assert!(events.iter().any(|event| event.event_type() == STALE_ACTIVE_RELEASE_EVENT));
	assert!(mapping.is_none());
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
	);
}

#[test]
fn stale_active_release_revalidates_needs_attention_before_final_label_removal() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let needs_attention_label =
		context.workflow.frontmatter().tracker().needs_attention_label().to_owned();
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&context.config.worktree_root().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = FinalNeedsAttentionTracker::new(issue, needs_attention_label);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("initial stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	let error = super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late needs-attention should block active-label release");
	let message = error.to_string();

	assert!(message.contains("safety inspection changed before apply"));
	assert!(message.contains("needs_attention_label_present"));
	assert!(
		tracker.label_removals.borrow().is_empty(),
		"active label should not be removed after late needs-attention appears"
	);
}

#[test]
fn stale_active_release_preflight_rejects_worktree_progress_after_diagnosis() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);
	let worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	fs::write(worktree_path.join("late_progress.rs"), "fn late_progress() {}\n")
		.expect("late untracked progress should write");
	let error =
		super::super::preflight_stale_active_worktree_cleanup(&context.state_store, &diagnostic)
			.expect_err("preflight should reject late retained progress");

	assert!(
		error.to_string().contains("retained worktree changes appeared before cleanup"),
		"unexpected preflight error: {error:?}"
	);
}

#[test]
fn stale_active_release_revalidates_late_default_worktree_progress_without_mapping() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
	let default_worktree_path = context.config.worktree_root().join("PUB-1626");

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	init_git_repo(&default_worktree_path);
	fs::write(default_worktree_path.join("late_default_progress.rs"), "fn late() {}\n")
		.expect("late default progress should write");
	let error = super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late default worktree progress should block release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");

	assert!(
		error.to_string().contains("safety inspection changed before apply"),
		"unexpected release error: {error:?}"
	);
	assert_eq!(run.status(), "running");
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn stale_active_release_revalidates_late_run_lease_before_mutation() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&context.config.worktree_root().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	context
		.state_store
		.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
		.expect("late lease should record");

	let error = super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late run lease should block release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");

	assert!(
		error.to_string().contains("safety inspection changed before apply"),
		"unexpected release error: {error:?}"
	);
	assert_eq!(run.status(), "running");
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn stale_active_release_revalidates_late_review_policy_before_mutation() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			"x/pubfi-pub-1626",
			&context.config.worktree_root().join("PUB-1626").display().to_string(),
		)
		.expect("worktree mapping should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	context
		.state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: context.config.service_id(),
			issue_id: &issue.id,
			run_id: "run-1626",
			attempt_number: 1,
			phase: "handoff",
			review_level: "normal",
			status: "clean",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("late review checkpoint should record");

	let error = super::super::apply_stale_active_release_with_tracker(
		&tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("late review checkpoint should block release");
	let run = context
		.state_store
		.run_attempt("run-1626")
		.expect("run attempt should read")
		.expect("run should exist");

	assert!(
		error.to_string().contains("safety inspection changed before apply")
			|| error.to_string().contains("review authority appeared"),
		"unexpected release error: {error:?}"
	);
	assert_eq!(run.status(), "running");
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn stale_active_final_label_guard_rejects_late_run_lease() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context = sample_recovery_context(
		&temp_dir,
		super::super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	);
	let active_label = tracker::automation_active_label(context.config.service_id());
	let queue_label = tracker::automation_queue_label(context.config.service_id());
	let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

	issue.identifier = String::from("PUB-1626");
	context
		.state_store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let mut diagnostics = super::super::diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.pop().expect("diagnostic should exist");

	assert!(diagnostic.recoverable());

	context
		.state_store
		.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
		.expect("late lease should record");

	let error = super::super::ensure_stale_active_run_claim_guard(
		&context.config,
		&context.state_store,
		&diagnostic,
	)
	.expect_err("final guard should reject late lease");

	assert!(
		error.to_string().contains("appeared before active-label release"),
		"unexpected final guard error: {error:?}"
	);
}

#[test]
fn stale_active_diagnose_blocks_when_run_lease_is_present() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let store = StateStore::open_in_memory().expect("state store should open");
	let workflow = sample_workflow();
	let active_label = tracker::automation_active_label("pubfi");
	let mut issue = sample_issue_with_labels("Todo", &[active_label]);

	issue.identifier = String::from("PUB-1626");
	store
		.record_run_attempt("run-1626", &issue.id, 1, "running")
		.expect("run attempt should record");
	store.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress").expect("lease should record");

	let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
	let diagnostics = super::super::diagnose_stale_active_issues(
		"pubfi",
		&workflow,
		temp_dir.path(),
		&store,
		&tracker,
		Some("PUB-1626"),
		super::super::RecoveryRuntimeMutationPolicy::ReadOnly,
	)
	.expect("stale active diagnosis should run");
	let diagnostic = diagnostics.first().expect("diagnostic should exist");

	assert_eq!(diagnostic.classification, super::super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
	assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
	assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
	assert!(!diagnostic.recoverable());
}
