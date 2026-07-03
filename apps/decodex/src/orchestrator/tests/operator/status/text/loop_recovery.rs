use crate::orchestrator::tests::operator::status::{
	self, ReviewPolicyCheckpointInput, ServiceConfig, StateStore, Value, orchestrator,
};

#[test]
fn operator_status_json_and_text_surface_loop_review_and_recovery_state() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	seed_loop_status_runs(&state_store, &config);
	seed_loop_status_review_checkpoints(&state_store, &config);
	seed_loop_status_private_events(&state_store, &config);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let pending = status_run_json(&snapshot_json, "run-pending");
	let clean = status_run_json(&snapshot_json, "run-clean");
	let findings = status_run_json(&snapshot_json, "run-findings");
	let blocked = status_run_json(&snapshot_json, "run-blocked");

	assert_eq!(pending["loop_status"]["review_level"], "strict");
	assert_eq!(pending["loop_status"]["review"]["status"], "pending");
	assert_eq!(clean["loop_status"]["review"]["status"], "clean");
	assert_eq!(
		clean["loop_status"]["review"]["checkpoint"]["head_sha"],
		"1111111111111111111111111111111111111111"
	);
	assert_eq!(findings["loop_status"]["review"]["status"], "findings");
	assert_eq!(findings["loop_status"]["review"]["checkpoint"]["round"], 2);
	assert_eq!(findings["loop_status"]["architecture_recovery"]["status"], "active");
	assert_eq!(findings["loop_status"]["autonomy"], "autonomous");
	assert_eq!(blocked["loop_status"]["review"]["status"], "blocked");
	assert_eq!(blocked["loop_status"]["architecture_recovery"]["status"], "exhausted");
	assert_eq!(blocked["loop_status"]["boundary"]["disposition"], "requires_human");
	assert_eq!(blocked["loop_status"]["boundary"]["policy_decision"], "requires_human_decision");
	assert_eq!(blocked["loop_status"]["autonomy"], "human_required");
	assert_eq!(blocked["loop_status"]["decision_request"]["decision_request_id"], "dr-pub-874-1");

	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains(
		"loop_review: phase=handoff status=pending checkpoint=head:0000000000000000000000000000000000000000 round:0"
	));
	assert!(rendered.contains("loop_review: phase=handoff status=findings checkpoint=head:2222222222222222222222222222222222222222 round:2"));
	assert!(rendered.contains(
		"loop_architecture_recovery: status=active reason=architecture_recovery_started"
	));
	assert!(rendered.contains(
		"loop_status: human-required boundary stop: contract_boundary_required on accepted_behavior; review_level=strict; autonomy=human_required"
	));
	assert!(rendered.contains(
		"loop_boundary: disposition=requires_human policy=requires_human_decision enhanced_evidence=false blocks_landing=false reason=accepted behavior would change attempted_recovery=review_churn"
	));
}

fn seed_loop_status_runs(state_store: &StateStore, config: &ServiceConfig) {
	for (issue_id, run_id) in [
		("issue-pending", "run-pending"),
		("issue-clean", "run-clean"),
		("issue-findings", "run-findings"),
		("issue-blocked", "run-blocked"),
	] {
		state_store
			.record_run_attempt(run_id, issue_id, 1, "running")
			.expect("run attempt should record");
		state_store
			.upsert_lease(config.service_id(), issue_id, run_id, "In Progress")
			.expect("lease should record");
	}
}

fn seed_loop_status_review_checkpoints(state_store: &StateStore, config: &ServiceConfig) {
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-pending",
			run_id: "run-pending",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "pending",
			head_sha: "0000000000000000000000000000000000000000",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("pending checkpoint should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-clean",
			run_id: "run-clean",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "clean",
			head_sha: "1111111111111111111111111111111111111111",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("clean checkpoint should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-findings",
			run_id: "run-findings",
			attempt_number: 1,
			phase: "handoff",
			review_level: config.codex().review_level().as_str(),
			status: "findings",
			head_sha: "2222222222222222222222222222222222222222",
			nonclean_rounds: 2,
			details_json: "{}",
		})
		.expect("findings checkpoint should record");
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: "issue-blocked",
			run_id: "run-blocked",
			attempt_number: 1,
			phase: "repair",
			review_level: config.codex().review_level().as_str(),
			status: "blocked",
			head_sha: "3333333333333333333333333333333333333333",
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("blocked checkpoint should record");
}

fn seed_loop_status_private_events(state_store: &StateStore, config: &ServiceConfig) {
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-findings",
			"run-findings",
			1,
			"architecture_recovery_started",
			serde_json::json!({
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": "validation_repeat",
				"recovery_budget": { "attempt": 1, "max_attempts": 2 },
			}),
		)
		.expect("active recovery should record");
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-blocked",
			"run-blocked",
			1,
			"authority_boundary_check",
			serde_json::json!({
				"attempted_recovery_reason": "review_churn",
				"disposition": "requires_human",
				"final_disposition": {
					"disposition": "requires_human",
					"reason": "accepted behavior would change",
				},
				"changed_surfaces": [{ "surface": "runtime", "change_summary": "change behavior" }],
				"improvement_signals": [{ "kind": "missing_validator" }],
			}),
		)
		.expect("boundary check should record");
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-blocked",
			"run-blocked",
			1,
			"architecture_recovery_terminal",
			serde_json::json!({
				"reason_code": "architecture_recovery_exhausted",
				"guardrail_reason": "review_churn",
				"boundary_disposition": "requires_human",
				"recovery_budget": { "attempt": 2, "max_attempts": 2 },
			}),
		)
		.expect("terminal recovery should record");
	state_store
		.append_private_execution_event(
			config.service_id(),
			"issue-blocked",
			"run-blocked",
			1,
			"authority_decision_request",
			serde_json::json!({
				"decision_request_id": "dr-pub-874-1",
				"phase": "human_required",
				"reason": "contract_boundary_required",
				"boundary": "accepted_behavior",
				"next_action": "accept or reject the recovery direction",
			}),
		)
		.expect("decision request should record");
}

fn status_run_json<'a>(snapshot_json: &'a Value, run_id: &str) -> &'a Value {
	for collection in ["current_lanes", "recent_runs"] {
		if let Some(run) = snapshot_json[collection]
			.as_array()
			.expect("status run collection should be an array")
			.iter()
			.find(|run| run["run_id"] == run_id)
		{
			return run;
		}
	}

	status::panic!("status run `{run_id}` should exist")
}
