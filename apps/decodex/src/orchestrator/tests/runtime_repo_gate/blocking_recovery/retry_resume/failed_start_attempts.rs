use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		IssueDispatchMode, IssueRunPlan, PhaseGoalKind, RepoGatePhaseGoalController, StateStore,
		tests, tests::TEST_SERVICE_ID,
	},
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn retry_phase_goal_skips_empty_failed_start_attempt_for_cross_attempt_resume() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: false,
	};

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-1",
			1,
			"phase_goal_next",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"reason": "validation_pass",
			}),
		)
		.expect("older handoff phase should record");
	state_store
		.record_run_attempt("pub-101-attempt-2", &issue.id, 2, "failed")
		.expect("empty previous attempt should record");

	let retry_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 2,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &retry_issue_run,
	}
	.initial_phase_goal()
	.expect("retry phase goal should build")
	.expect("retry should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::HandoffEvidence);
	assert!(
		goal.objective.contains("prepare PR-backed handoff evidence"),
		"empty failed-start attempts must not erase the issue's open handoff phase"
	);
}

#[test]
fn program_phase_goal_skips_empty_failed_start_attempt_for_cross_attempt_resume() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: false,
	};

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-1",
			1,
			"phase_goal_next",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"reason": "validation_pass",
			}),
		)
		.expect("older handoff phase should record");
	state_store
		.record_run_attempt("pub-101-attempt-2", &issue.id, 2, "failed")
		.expect("empty previous attempt should record");

	let program_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree,
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Program,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3"),
		retry_budget_base: 2,
	};
	let goal = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &program_issue_run,
	}
	.initial_phase_goal()
	.expect("program phase goal should build")
	.expect("program run should still set a phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::HandoffEvidence);
	assert!(
		goal.objective.contains("prepare PR-backed handoff evidence"),
		"program dispatch must continue the open handoff phase instead of restarting implementation"
	);
}
