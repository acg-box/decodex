use std::fs;

use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		IssueDispatchMode, IssueRunPlan, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PhaseGoalKind,
		PhaseGoalSpec, PhaseGoalTransition, RepoGatePhaseGoalController, StateStore, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn phase_goal_completion_continues_with_owned_tracked_rewrites_after_validation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 3)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > ready.txt\"]",
			)
			.replace(
				"verify_commands = []",
				"verify_commands = [\"grep -qx rewritten ready.txt\"]",
			),
	);
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
	support::record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("owned tracked canonicalize rewrites should satisfy phase validation");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	match transition {
		PhaseGoalTransition::Continue(PhaseGoalSpec {
			phase: PhaseGoalKind::HandoffEvidence,
			objective,
			..
		}) => {
			assert!(objective.contains("ready.txt"));
			assert!(objective.contains("Commit these issue-owned gate rewrites"));
		},
		_ => panic!("owned tracked rewrites should continue to handoff evidence"),
	}

	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_pass"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == true
			&& event.payload()["payload"]["trackedRewrites"]["decision"]
				== "continue_to_commit_capable_phase"
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "pass"
			&& event.payload()["validation_evidence"]["tracked_rewrites"]["files"]
				.as_array()
				.is_some_and(|files| files.iter().any(|file| file.as_str() == Some("ready.txt")))
	}));
}

#[test]
fn phase_goal_acceptance_accepts_committed_branch_delta_with_clean_worktree() {
	let (temp_dir, config, workflow) = tests::temp_project_layout();
	let remote_root = temp_dir.path().join("origin.git");
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

	tests::add_origin_remote(config.repo_root(), &remote_root);
	tests::checkout_new_branch(config.repo_root(), &issue_run.worktree.branch_name);
	tests::commit_worktree_change(
		config.repo_root(),
		"ready.txt",
		"implementation complete\n",
		"implement issue scope",
	);

	assert_eq!(tests::git_output(config.repo_root(), &["status", "--porcelain"]), "");

	support::record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	let transition = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	}
	.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
	.expect("clean committed branch delta should satisfy phase acceptance");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private phase goal events should load");

	assert!(matches!(
		transition,
		PhaseGoalTransition::Continue(PhaseGoalSpec { phase: PhaseGoalKind::HandoffEvidence, .. })
	));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "pass"
			&& event.payload()["reason_code"] == "accepted"
			&& event.payload()["effective_delta"]["present"] == true
			&& event.payload()["effective_delta"]["changed_surfaces"].as_array().is_some_and(
				|surfaces| surfaces.iter().any(|surface| surface.as_str() == Some("ready.txt")),
			)
	}));
}
