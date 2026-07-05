use crate::{
	orchestrator::{
		PhaseGoalKind, StateStore, execution_phase_goal, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
};

#[test]
fn cleared_checkpoint_allows_same_run_phase_goal_recovery_candidate() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = support::phase_goal_repo_gate_issue_run(&config, &issue);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"phase_goal_status",
			serde_json::json!({
				"phase": "implement_to_validation_ready",
				"status": "active",
			}),
		)
		.expect("phase goal status should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": ["repo-wide baseline requires separate authority"],
			}),
		)
		.expect("blocking checkpoint should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": [],
			}),
		)
		.expect("clearing checkpoint should record");

	assert_eq!(
		execution_phase_goal::latest_phase_goal_recovery_candidate(
			&config,
			&state_store,
			&issue_run,
		)
		.expect("phase goal recovery candidate should evaluate"),
		Some(PhaseGoalKind::ImplementToValidationReady)
	);
}
