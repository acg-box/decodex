mod operator_status_current_lane_lifecycle_reconstructs_all_issue_attempts;
mod operator_status_supersedes_stale_repair_findings_after_clean_handoff_checkpoint;

use crate::orchestrator::tests::operator::status::running_lanes::{
	OperatorRunStatus, ReviewCheckpointSeed, ReviewPolicyCheckpointInput, ServiceConfig,
	StateStore, TEST_SERVICE_ID,
};

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
