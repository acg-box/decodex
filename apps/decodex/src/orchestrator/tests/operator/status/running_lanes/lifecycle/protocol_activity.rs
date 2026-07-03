use crate::orchestrator::tests::operator::status::running_lanes::{
	self, OffsetDateTime, ProtocolActivityMarker, ProtocolActivitySummary,
	ReviewPolicyCheckpointInput, StateStore, TEST_SERVICE_ID, fs, lifecycle::shared, orchestrator,
	process, state, tracker,
};
#[test]
fn operator_status_current_lane_lifecycle_recovers_from_local_evidence_after_restart() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let worktree_path = config.worktree_root().join("PUB-101");
	let development_activity = shared::sample_lifecycle_activity(480, 4, 2, 600, 120);
	let review_activity = shared::sample_lifecycle_activity(240, 3, 1, 300, 90);

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
	assert!(run.lifecycle_metrics.attempt_evidence.iter().any(|attempt| {
		attempt.run_id == "run-development"
			&& attempt.source == "recovered"
			&& attempt
				.evidence
				.iter()
				.any(|evidence| evidence == "private_execution_event:issue_progress_checkpoint")
	}));
	assert!(
		run.lifecycle_metrics.attempt_evidence.iter().any(|attempt| attempt.run_id == "run-review"
			&& attempt.source == "current_snapshot"
			&& attempt.evidence.iter().any(|evidence| evidence == "worktree_activity_marker"))
	);
}

#[test]
fn operator_status_snapshot_uses_structured_protocol_activity_summary() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	assert_eq!(
		snapshot.projects[0].waiting_lane_count, 1,
		"approval or user-input waits should remain project-level waiting"
	);
	assert!(rendered.contains("protocol_activity: turn=running; waiting=approval_or_user_input; rate_limit=primary; recent=item/tool/requestUserInput, plan/update:verify"));
}

#[test]
fn operator_status_snapshot_prefers_newer_protocol_marker_over_stale_archive_event() {
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
	state_store
		.append_event("run-1", 1, "thread/archive/discarded", "{}")
		.expect("archive event should record");

	state::write_run_protocol_activity_marker(
		&worktree_path,
		&ProtocolActivityMarker {
			run_id: "run-1",
			attempt_number: 1,
			thread_id: Some("thread-1"),
			turn_id: Some("turn-1"),
			event_count: 2,
			last_event_type: "item/tool/call",
			child_agent_activity: None,
			protocol_activity: None,
		},
	)
	.expect("protocol activity marker should write");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");

	assert_eq!(run.last_event_type.as_deref(), Some("item/tool/call"));
	assert_eq!(run.event_count, 2);
}

#[test]
fn operator_status_snapshot_sanitizes_private_protocol_activity_details() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
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
