use crate::orchestrator::tests::operator::status::running_lanes::{
	self, ChildAgentActivitySummary, OperatorRunStatus, ReviewCheckpointSeed,
	ReviewPolicyCheckpointInput, ServiceConfig, StateStore, TEST_SERVICE_ID, orchestrator, state,
	tracker,
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

	assert_compact_review_checkpoint_status(run);
}

#[test]
fn operator_status_supersedes_stale_repair_findings_after_clean_handoff_checkpoint() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let run_id = "run-review";
	let repair_head = "1111111111111111111111111111111111111111";
	let clean_head = "2222222222222222222222222222222222222222";

	state_store
		.record_run_attempt(run_id, &issue.id, 2, "running")
		.expect("review attempt should record");
	state_store
		.upsert_lease(TEST_SERVICE_ID, &issue.id, run_id, "In Progress")
		.expect("lease should record");

	let stale_repair_next_action = seed_stale_repair_and_clean_handoff_checkpoints(
		&state_store,
		&config,
		&issue.id,
		run_id,
		repair_head,
		clean_head,
	);
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 0)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");
	let review = loop_status.review.as_ref().expect("review status should render");
	let checkpoint = review.checkpoint.as_ref().expect("review checkpoint should render");

	assert_eq!(review.phase, "handoff");
	assert_eq!(review.status, "clean");
	assert_eq!(checkpoint.head_sha, clean_head);
	assert!(checkpoint.active_fingerprints.is_empty());
	assert_eq!(run.policy_state, "allowed");
	assert_eq!(
		run.lane_control_next_action,
		"Push or update the PR and record review handoff for the clean current lane head."
	);
	assert_ne!(loop_status.next_action.as_deref(), Some(stale_repair_next_action));
}

fn seed_stale_repair_and_clean_handoff_checkpoints(
	state_store: &StateStore,
	config: &ServiceConfig,
	issue_id: &str,
	run_id: &str,
	repair_head: &str,
	clean_head: &str,
) -> &'static str {
	let stale_repair_next_action = "Repair the stale review finding.";
	let repair_details_json = r#"{
		"finding_route_summary": {
			"route_counts": [{"route": "current_blocker", "count": 1}],
			"next_action": "Repair the stale review finding."
		},
		"finding_policy": {
			"active_fingerprints": ["stale-finding"],
			"stop_fingerprint": null
		}
	}"#;
	let clean_details_json = r#"{
		"review_cost_control": {
			"review_class": "full_current_head_review",
			"risk_class": "localized",
			"compact_eligible": false,
			"fallback_reason": "repair_review"
		},
		"finding_route_summary": {
			"route_counts": [],
			"next_action": null
		},
		"finding_policy": {
			"active_fingerprints": [],
			"stop_fingerprint": null
		}
	}"#;

	seed_review_policy_checkpoint_with_event(
		state_store,
		config,
		ReviewCheckpointSeed {
			issue_id,
			run_id,
			phase: "repair",
			status: "findings",
			head_sha: repair_head,
			nonclean_rounds: 1,
			details_json: repair_details_json,
		},
	);
	seed_review_policy_checkpoint_with_event(
		state_store,
		config,
		ReviewCheckpointSeed {
			issue_id,
			run_id,
			phase: "handoff",
			status: "clean",
			head_sha: clean_head,
			nonclean_rounds: 0,
			details_json: clean_details_json,
		},
	);
	seed_review_policy_checkpoint(
		state_store,
		config,
		ReviewCheckpointSeed {
			issue_id,
			run_id,
			phase: "repair",
			status: "findings",
			head_sha: repair_head,
			nonclean_rounds: 1,
			details_json: repair_details_json,
		},
	);

	stale_repair_next_action
}

fn seed_review_policy_checkpoint_with_event(
	state_store: &StateStore,
	config: &ServiceConfig,
	seed: ReviewCheckpointSeed<'_>,
) {
	seed_review_policy_checkpoint(state_store, config, seed);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			seed.issue_id,
			seed.run_id,
			2,
			"review_checkpoint",
			serde_json::json!({
				"phase": seed.phase,
				"status": seed.status,
				"head_sha": seed.head_sha,
				"nonclean_rounds": seed.nonclean_rounds
			}),
		)
		.expect("review checkpoint event should record");
}

fn seed_review_policy_checkpoint(
	state_store: &StateStore,
	config: &ServiceConfig,
	seed: ReviewCheckpointSeed<'_>,
) {
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id: seed.issue_id,
			run_id: seed.run_id,
			attempt_number: 2,
			phase: seed.phase,
			review_level: config.codex().review_level().as_str(),
			status: seed.status,
			head_sha: seed.head_sha,
			nonclean_rounds: seed.nonclean_rounds,
			details_json: seed.details_json,
		})
		.expect("review policy checkpoint should record");
}

fn assert_compact_review_checkpoint_status(run: &OperatorRunStatus) {
	let review_checkpoint = run
		.loop_status
		.as_ref()
		.and_then(|loop_status| loop_status.review.as_ref())
		.and_then(|review| review.checkpoint.as_ref())
		.expect("review checkpoint should render in loop status");

	assert_eq!(review_checkpoint.route_counts[0].route, "risk_note");
	assert_eq!(review_checkpoint.route_counts[0].count, 1);
	assert_eq!(review_checkpoint.review_class.as_deref(), Some("compact_current_head_review"));
	assert_eq!(review_checkpoint.risk_class.as_deref(), Some("low"));
	assert_eq!(review_checkpoint.compact_eligible, Some(true));
	assert_eq!(review_checkpoint.fallback_reason, None);
	assert_eq!(
		review_checkpoint.route_next_action.as_deref(),
		Some("Carry the routed risk note into follow-up planning.")
	);
}
