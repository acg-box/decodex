use std::fs;

use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		IssueDispatchMode, IssueRunPlan, ManualAttentionRequested,
		PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition,
		RepoGatePhaseGoalController, StateStore, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn phase_goal_acceptance_rejects_repo_gate_pass_without_effective_delta() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.repo_root().to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};

	support::record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("repo gate pass should still record acceptance failure");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	assert!(matches!(
		transition,
		PhaseGoalTransition::Continue(PhaseGoalSpec {
			phase: PhaseGoalKind::RepairValidationFailures,
			..
		})
	));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "fail"
			&& event.payload()["reason_code"] == "no_effective_delta"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_fail"
			&& event.payload()["payload"]["errorClass"] == "phase_acceptance_check_failed"
			&& event.payload()["payload"]["laneDecision"] == "retry_failure"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["next_action"] == "retry_failure"
			&& event.payload()["phase_acceptance_failure"] == true
	}));
	assert!(events.iter().all(|event| {
		event.event_type() != "phase_goal_next" || event.payload()["phase"] != "handoff_evidence"
	}));
}

#[test]
fn phase_goal_acceptance_non_goal_violation_requests_manual_attention() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.repo_root().to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");
	support::record_phase_acceptance_progress_checkpoint(
		&config,
		&state_store,
		&issue_run,
		&["non-goal violation: changed retained ownership policy"],
	);

	let error = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect_err("non-goal acceptance failure should stop automatic repair");
	let manual_attention = error
		.downcast_ref::<ManualAttentionRequested>()
		.expect("non-goal acceptance failure should request manual attention");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	assert_eq!(manual_attention.error_class.as_deref(), Some("phase_acceptance_check_failed"));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "fail"
			&& event.payload()["reason_code"] == "non_goal_violation"
			&& event.payload()["non_goal_check"]["passed"] == false
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["next_action"] == "needs_attention"
			&& event.payload()["non_goal_violation"] == true
	}));
}
