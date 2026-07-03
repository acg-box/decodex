use std::fs;

use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, PHASE_GOAL_RECOVERY_EVENT_TYPE, PhaseGoalKind,
		RepoGatePhaseGoalController, StateStore, execution_phase_goal, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn blocking_lane_decision_evidence_clears_after_new_unblocked_checkpoint() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-cleared-blocker";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": ["repo-wide baseline requires separate authority"],
				"docs_impact": "none",
			}),
		)
		.expect("blocking checkpoint should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate")
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"docs_impact": "none",
			}),
		)
		.expect("ordinary checkpoint should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"checkpoint without an explicit empty blockers array must not clear older blockers"
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": [],
				"docs_impact": "none",
			}),
		)
		.expect("clearing checkpoint should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"latest unblocked checkpoint should clear older progress blockers"
	);
}

#[test]
fn blocking_lane_decision_evidence_prefers_kernel_projection_over_legacy_action() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-kernel-lane-decision";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"lane_decision",
			serde_json::json!({
				"next_action": "needs_attention",
				"kernel_decision": {
					"decision_class": "retry_automatically",
					"command_intents": [{"kind": "schedule_retry"}],
				},
			}),
		)
		.expect("kernel retry decision should record");

	assert!(
		!orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"kernel decision must override stale compatibility action"
	);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			1,
			"lane_decision",
			serde_json::json!({
				"next_action": "retry_failure",
				"kernel_decision": {
					"decision_class": "manual_intervention_required",
					"command_intents": [{"kind": "request_manual_intervention"}],
				},
			}),
		)
		.expect("kernel manual decision should record");

	assert!(
		orchestrator::issue_has_blocking_lane_decision_evidence(&config, &state_store, &issue.id)
			.expect("blocking evidence should evaluate"),
		"kernel manual decision must block even when compatibility action is stale"
	);
}

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

#[test]
fn cleared_checkpoint_allows_cross_attempt_phase_goal_inheritance() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let source_run_id = "pub-101-attempt-1";

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
			"phase_goal_next",
			serde_json::json!({
				"phase": "handoff_evidence",
			}),
		)
		.expect("phase goal next should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			source_run_id,
			1,
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
			source_run_id,
			1,
			"progress_checkpoint",
			serde_json::json!({
				"blockers": [],
			}),
		)
		.expect("clearing checkpoint should record");

	assert_eq!(
		orchestrator::latest_open_issue_phase_goal_before_attempt(
			&config,
			&state_store,
			&issue.id,
			"pub-101-attempt-2",
			2,
		)
		.expect("phase goal inheritance should evaluate"),
		Some(PhaseGoalKind::HandoffEvidence)
	);
}

#[test]
fn retry_phase_goal_resumes_cross_attempt_handoff_after_recovered_validation_pass() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let first_issue_run = IssueRunPlan {
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
		&first_issue_run,
		&[],
	);

	RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &first_issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("completed implementation phase should persist handoff phase");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&first_issue_run.run_id,
			first_issue_run.attempt_number,
			PHASE_GOAL_RECOVERY_EVENT_TYPE,
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"signal": "phase_goal_recovered",
				"payload": {
					"nextPhase": "handoff_evidence",
					"sourceErrorClass": "app_server_run_failed",
				},
			}),
		)
		.expect("phase goal recovery should record");

	let retry_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: first_issue_run.worktree.clone(),
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2"),
		retry_budget_base: 1,
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
		"retry should continue to handoff instead of repeating implementation"
	);
}

#[test]
fn retry_phase_goal_resumes_cross_attempt_active_handoff_phase() {
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
			"pub-101-attempt-2",
			2,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"payload": {
					"phase": "handoff_evidence",
					"status": "active",
				},
			}),
		)
		.expect("active handoff phase should record");

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
		"retry should resume handoff evidence instead of repeating implementation"
	);
}

#[test]
fn retry_phase_goal_does_not_resume_cross_attempt_phase_after_terminal_finalize() {
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
			"pub-101-attempt-2",
			2,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "handoff_evidence",
				"payload": {
					"phase": "handoff_evidence",
					"status": "active",
				},
			}),
		)
		.expect("active handoff phase should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2",
			2,
			"terminal_finalize",
			serde_json::json!({
				"schema": "decodex.terminal_finalize/1",
				"path": "review_handoff",
			}),
		)
		.expect("terminal finalize should record");

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

	assert_eq!(goal.phase, PhaseGoalKind::ImplementToValidationReady);
}

#[test]
fn retry_phase_goal_uses_latest_open_phase_for_cross_attempt_resume() {
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
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2",
			2,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("newer implementation phase should record");

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

	assert_eq!(goal.phase, PhaseGoalKind::ImplementToValidationReady);
}

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
