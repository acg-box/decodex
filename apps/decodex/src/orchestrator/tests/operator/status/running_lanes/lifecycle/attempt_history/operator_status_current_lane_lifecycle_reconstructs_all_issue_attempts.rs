use crate::orchestrator::tests::operator::status::running_lanes::{
	self, ChildAgentActivitySummary, ReviewPolicyCheckpointInput, StateStore, TEST_SERVICE_ID,
	lifecycle::attempt_history, orchestrator, state, tracker,
};

#[test]
fn operator_status_current_lane_lifecycle_reconstructs_all_issue_attempts() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
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
			details_json: r#"{
				"review_cost_control": {
					"review_class": "compact_current_head_review",
					"risk_class": "low",
					"compact_eligible": true,
					"fallback_reason": null
				},
				"finding_route_summary": {
					"route_counts": [{"route": "risk_note", "count": 1}],
					"next_action": "Carry the routed risk note into follow-up planning."
				},
				"finding_policy": {
					"active_fingerprints": [],
					"stop_fingerprint": null
				}
			}"#,
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

	attempt_history::assert_compact_review_checkpoint_status(run);
}
