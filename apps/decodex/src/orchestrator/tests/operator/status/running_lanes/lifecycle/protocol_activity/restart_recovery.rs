use crate::orchestrator::tests::operator::status::running_lanes::{
	self, ProtocolActivityMarker, ReviewPolicyCheckpointInput, StateStore, TEST_SERVICE_ID,
	lifecycle::shared, orchestrator, state, tracker,
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
