use std::fs;

use crate::{
	agent::PhaseGoalController,
	lane_authority::{LaneCommand, LaneId, NoEffectiveDeltaRecoveryState},
	orchestrator::{
		IssueDispatchMode, IssueRunPlan, ManualAttentionRequested, PhaseGoalKind, PhaseGoalSpec,
		PhaseGoalTransition, RepoGatePhaseGoalController, StateStore,
		VALIDATION_EVIDENCE_EVENT_TYPE, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn lane_authority_v2_c6_adj_01() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let lane_id = LaneId::new(TEST_SERVICE_ID, &issue.id).expect("lane");
	let base_oid = tests::git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	state_store
		.transition_lane(
			lane_id.clone(),
			0,
			"binding-1",
			LaneCommand::Admit { intake_authority_id: String::from("authority-1") },
		)
		.expect("admit lane");
	state_store
		.transition_lane(
			lane_id.clone(),
			1,
			"binding-1",
			LaneCommand::AcquireClaim { run_id: String::from("pub-101-attempt-1") },
		)
		.expect("claim lane");
	state_store
		.transition_lane(
			lane_id,
			2,
			"binding-1",
			LaneCommand::FreezeAdmittedBase { oid: base_oid },
		)
		.expect("freeze base");
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

	support::record_validation_evidence_progress_checkpoint(&config, &state_store, &issue_run, &[]);

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
		PhaseGoalTransition::ScheduleContinuation(PhaseGoalSpec {
			phase: PhaseGoalKind::RepairValidationFailures,
			..
		})
	));
	assert!(events.iter().any(|event| {
		event.event_type() == VALIDATION_EVIDENCE_EVENT_TYPE
			&& event.payload()["decision"] == "fail"
			&& event.payload()["reason_code"] == "no_effective_delta"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "no_effective_delta_retry_scheduled"
			&& event.payload()["payload"]["ordinal"] == 1
	}));
	assert!(events.iter().all(|event| {
		event.event_type() != "phase_goal_next" || event.payload()["phase"] != "handoff_evidence"
	}));

	let retry_run = IssueRunPlan {
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2"),
		..issue_run.clone()
	};
	support::record_validation_evidence_progress_checkpoint(
		&config,
		&state_store,
		&retry_run,
		&[],
	);
	let error = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &retry_run,
	}
	.phase_goal_completed(PhaseGoalKind::RepairValidationFailures)
	.expect_err("second no-effective-delta result must require attention");
	let attention = error.downcast_ref::<ManualAttentionRequested>().expect("manual attention");
	assert_eq!(attention.error_class.as_deref(), Some("no_effective_delta_unresolved"));
	let operation_id = events
		.iter()
		.find(|event| {
			event.event_type() == "phase_goal_transition"
				&& event.payload()["signal"] == "no_effective_delta_retry_scheduled"
		})
		.and_then(|event| event.payload()["payload"]["operationId"].as_str())
		.expect("operation id");
	let recovery = state_store
		.no_effective_delta_recovery(operation_id)
		.expect("read recovery")
		.expect("recovery");
	assert_eq!(recovery.state(), NoEffectiveDeltaRecoveryState::AttentionRequired);
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
	support::record_validation_evidence_progress_checkpoint(
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

	assert_eq!(manual_attention.error_class.as_deref(), Some("validation_evidence_failed"));
	assert!(events.iter().any(|event| {
		event.event_type() == VALIDATION_EVIDENCE_EVENT_TYPE
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
