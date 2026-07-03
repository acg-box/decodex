use crate::orchestrator::tests::operator::status::running_lanes::{
	self, RUN_OPERATION_AGENT_RUN, ServiceConfig, StateStore, TrackerIssue, orchestrator, state,
};

#[test]
fn operator_status_snapshot_prioritizes_repo_gate_progress_diagnostic() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);

	seed_running_repo_gate_status_lane(&state_store, &config, &issue);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(
		run.progress_diagnostic.as_deref(),
		Some("repo_gate_failure:repo_gate_canonicalize_failed; failed_command:cargo make lint-fix")
	);
	assert!(
		rendered.contains(
			"progress_diagnostic: repo_gate_failure:repo_gate_canonicalize_failed; failed_command:cargo make lint-fix"
		)
	);
}

#[test]
fn operator_status_snapshot_clears_repo_gate_progress_after_later_transition() {
	let (_temp_dir, config, _workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);

	seed_running_repo_gate_status_lane(&state_store, &config, &issue);

	state_store
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"phase_goal_transition",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "repair_validation_failures",
				"signal": "validation_pass",
				"payload": {
					"nextPhase": "handoff_evidence"
				}
			}),
		)
		.expect("validation pass event should record");

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert_eq!(run.progress_diagnostic, None);
	assert!(!rendered.contains("repo_gate_failure:repo_gate_canonicalize_failed"));
}

fn seed_running_repo_gate_status_lane(
	state_store: &StateStore,
	config: &ServiceConfig,
	issue: &TrackerIssue,
) {
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
		.append_private_execution_event(
			"pubfi",
			&issue.id,
			"run-1",
			1,
			"phase_goal_transition",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"signal": "validation_fail",
				"payload": {
					"errorClass": "repo_gate_canonicalize_failed",
					"disposition": "continue_repair",
					"repoGateFailure": {
						"schema": "decodex.repo_gate_failure_diagnostic/1",
						"stage": "canonicalize",
						"failed_command": "cargo make lint-fix",
						"exit_status": 101,
						"summary": "repo gate canonicalize command failed",
						"problem_lines": ["error: function has too many lines"],
						"output_excerpt": "error: function has too many lines",
						"output_truncated": false
					}
				}
			}),
		)
		.expect("repo gate failure event should record");

	state::write_run_operation_marker(&worktree_path, "run-1", 1, RUN_OPERATION_AGENT_RUN)
		.expect("operation marker should write");
}
