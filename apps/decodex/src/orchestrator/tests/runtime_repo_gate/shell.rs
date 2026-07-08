use std::{ffi::OsString, path::Path};

use crate::{
	agent::PhaseGoalController,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, PhaseGoalKind, RepoGatePhaseGoalController,
		StateStore, tests, tests::TEST_SERVICE_ID,
	},
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn implementation_phase_goal_contract_avoids_checkpoint_ceremony() {
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
	let controller = RepoGatePhaseGoalController {
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_run: &issue_run,
	};
	let goal = controller
		.initial_phase_goal()
		.expect("phase goal should build")
		.expect("normal dispatch should set an implementation phase goal");

	assert_eq!(goal.phase, PhaseGoalKind::ImplementToValidationReady);
	assert!(goal.objective.contains("Decodex step: implementation"));
	assert!(goal.objective.contains("mark the active phase goal complete"));
	assert!(goal.objective.contains("Decodex owns repository validation"));
	assert!(!goal.objective.contains("issue_progress_checkpoint"));
}

#[test]
fn repo_gate_shell_falls_back_to_non_login_posix_sh_for_missing_absolute_shell() {
	let (shell, shell_flag) = orchestrator::repo_gate_shell_from_env(Some(OsString::from(
		"/definitely-missing-shell-for-tests",
	)));

	assert_eq!(Path::new(&shell), Path::new("/bin/sh"));
	assert_eq!(shell_flag, "-c");
}

#[test]
fn repo_gate_shell_uses_non_login_mode_when_shell_is_bin_sh() {
	let (shell, shell_flag) =
		orchestrator::repo_gate_shell_from_env(Some(OsString::from("/bin/sh")));

	assert_eq!(Path::new(&shell), Path::new("/bin/sh"));
	assert_eq!(shell_flag, "-c");
}

#[test]
fn repo_gate_shell_keeps_login_mode_for_other_configured_shells() {
	let (shell, shell_flag) =
		orchestrator::repo_gate_shell_from_env(Some(OsString::from("/bin/bash")));

	assert_eq!(Path::new(&shell), Path::new("/bin/bash"));
	assert_eq!(shell_flag, "-lc");
}
