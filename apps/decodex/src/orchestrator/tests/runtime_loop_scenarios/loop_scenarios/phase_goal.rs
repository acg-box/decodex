use std::fs;

use crate::{
	agent::{PhaseGoalController, PhaseGoalKind, PhaseGoalSpec, PhaseGoalTransition},
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, RepoGatePhaseGoalController,
		tests::{
			self,
			runtime_loop_scenarios::loop_scenarios::support::{
				self, LOOP_SCENARIO_GATE_SERVICE_ID,
			},
			runtime_repo_gate,
		},
	},
	state::StateStore,
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn loop_scenario_phase_completion_runs_validation_and_guardrails_bound_repair() {
	loop_scenario_assert_phase_goal_completion_runs_validation();
	loop_scenario_assert_validation_guardrail_stops_after_threshold();
}

fn loop_scenario_assert_phase_goal_completion_runs_validation() {
	let workflow_markdown = tests::sample_workflow_markdown(
		"pubfi",
		&[],
		"Phase goal validation policy.\n",
		3,
	)
	.replace(
		"canonicalize_commands = []",
		"canonicalize_commands = [\"printf canonicalized > phase-canonicalized.txt\"]",
	)
	.replace(
		"verify_commands = []",
		"verify_commands = [\"test -f phase-canonicalized.txt && printf verified > phase-verified.txt\"]",
	);
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(LOOP_SCENARIO_GATE_SERVICE_ID).as_str()],
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
	runtime_repo_gate::record_phase_acceptance_progress_checkpoint(
		&config,
		&state_store,
		&issue_run,
		&[],
	);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("completed implementation phase should run validation");
	let events = state_store
		.list_private_execution_events(
			LOOP_SCENARIO_GATE_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			1,
		)
		.expect("phase goal events should load");

	assert!(config.repo_root().join("phase-canonicalized.txt").exists());
	assert!(config.repo_root().join("phase-verified.txt").exists());
	assert!(matches!(
		transition,
		PhaseGoalTransition::Continue(PhaseGoalSpec { phase: PhaseGoalKind::HandoffEvidence, .. })
	));
	assert!(!matches!(transition, PhaseGoalTransition::CompleteRun));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_pass"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_next" && event.payload()["phase"] == "handoff_evidence"
	}));
}

fn loop_scenario_assert_validation_guardrail_stops_after_threshold() {
	let (_guardrail_temp_dir, guardrail_config, _guardrail_workflow) = tests::temp_project_layout();
	let guardrail_store = StateStore::open_in_memory().expect("guardrail store should open");
	let guardrail_issue = tests::sample_issue("In Progress", &[]);

	for round in 1..=2 {
		let guardrail_issue_run =
			tests::loop_guardrail_issue_run(&guardrail_config, &guardrail_issue, round);
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&guardrail_config,
			&guardrail_store,
			&guardrail_issue_run,
			&support::loop_scenario_repo_gate_failure(),
		)
		.expect("guardrail observation should persist");

		assert!(stop.is_none(), "round {round} should keep repairing before the threshold");
	}

	let guardrail_issue_run =
		tests::loop_guardrail_issue_run(&guardrail_config, &guardrail_issue, 3);
	let stop = orchestrator::retryable_failure_loop_guardrail_stop(
		&guardrail_config,
		&guardrail_store,
		&guardrail_issue_run,
		&support::loop_scenario_repo_gate_failure(),
	)
	.expect("third guardrail observation should persist")
	.expect("third identical failure should stop repair churn");
	let checkpoint = guardrail_store
		.loop_guardrail_checkpoint(
			LOOP_SCENARIO_GATE_SERVICE_ID,
			&guardrail_issue_run.issue.id,
			"validation_repeat",
		)
		.expect("checkpoint lookup should succeed")
		.expect("validation repeat checkpoint should exist");
	let guardrail_events = guardrail_store
		.list_private_execution_events(
			LOOP_SCENARIO_GATE_SERVICE_ID,
			&guardrail_issue_run.issue.id,
			&guardrail_issue_run.run_id,
			guardrail_issue_run.attempt_number,
		)
		.expect("guardrail events should load");

	assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::ValidationRepeat);
	assert_eq!(checkpoint.consecutive_count(), 3);
	assert!(guardrail_events.iter().any(|event| {
		event.event_type() == "loop_guardrail_checkpoint"
			&& event.payload()["reason"] == "validation_repeat"
			&& event.payload()["consecutive_count"] == 3
	}));
}
