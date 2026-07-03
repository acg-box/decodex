use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, AppServerCapabilityPreflightFailure, IssueDispatchMode, IssueRunPlan,
		PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE,
		PHASE_GOAL_RECOVERY_EVENT_TYPE, RepoGateFailure, StateStore, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
	worktree::WorktreeSpec,
};

#[test]
fn open_phase_goal_unowned_tracked_rewrites_stop_instead_of_repair_continuation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 1)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > other.txt\"]",
			),
	);
	let repo_root = config.repo_root();
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
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};

	tests::commit_worktree_change(repo_root, "ready.txt", "before\n", "add ready file");
	tests::commit_worktree_change(repo_root, "other.txt", "before\n", "add other file");
	fs::write(repo_root.join("ready.txt"), "after\n").expect("tracked diff should write");
	support::record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			1,
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
		.expect("phase goal event should record");

	let error = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("app server transport closed after local verification"),
	)
	.expect_err("tracked repo-gate rewrites should stop phase-goal continuation");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private events should load");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("phase goal recovery should preserve repo-gate failure");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_tracked_rewrites_left");
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_fail"
			&& event.payload()["payload"]["disposition"] == "needs_human_attention"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == false
			&& event.payload()["payload"]["trackedRewrites"]["files"]
				.as_array()
				.is_some_and(|files| files.iter().any(|file| file.as_str() == Some("other.txt")))
	}));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_next"));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_recovery"));
}

#[test]
fn open_phase_goal_owned_tracked_rewrites_continue_to_handoff_recovery() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 1)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > ready.txt\"]",
			),
	);
	let repo_root = config.repo_root();
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
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};

	tests::commit_worktree_change(repo_root, "ready.txt", "before\n", "add ready file");
	fs::write(repo_root.join("ready.txt"), "after\n").expect("tracked diff should write");
	support::record_phase_acceptance_progress_checkpoint(&config, &state_store, &issue_run, &[]);

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			&issue_run.run_id,
			1,
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
		.expect("phase goal event should record");

	let summary = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&issue_run,
		&Report::msg("app server transport closed after local verification"),
	)
	.expect("owned tracked repo-gate rewrites should keep phase-goal recovery automatic")
	.expect("owned tracked repo-gate rewrites should schedule continuation");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private events should load");

	assert!(summary.continuation_pending);
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_pass"
			&& event.payload()["payload"]["nextPhase"] == "handoff_evidence"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == true
			&& event.payload()["payload"]["trackedRewrites"]["decision"]
				== "continue_to_commit_capable_phase"
			&& event.payload()["payload"]["trackedRewrites"]["files"]
				.as_array()
				.is_some_and(|files| files.iter().any(|file| file.as_str() == Some("ready.txt")))
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE
			&& event.payload()["decision"] == "pass"
			&& event.payload()["validation_evidence"]["tracked_rewrites"]["owned"] == true
	}));
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_next" && event.payload()["phase"] == "handoff_evidence"
	}));
	assert!(events.iter().any(|event| event.event_type() == "phase_goal_recovery"));
}

#[test]
fn repeated_phase_goal_recovery_blocks_second_automatic_continuation() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let repo_root = config.repo_root();
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let app_server_timeout =
		Report::new(AppServerCapabilityPreflightFailure::method_timed_out_for_test(
			"thread/goal/get",
			String::from("Timed out while waiting for app-server output."),
		));

	tests::commit_worktree_change(repo_root, "ready.txt", "before\n", "add ready file");
	fs::write(repo_root.join("ready.txt"), "after\n").expect("tracked diff should write");

	for (run_id, attempt_number) in [("pub-101-attempt-1", 1), ("pub-101-attempt-2", 2)] {
		state_store
			.append_private_execution_event(
				TEST_SERVICE_ID,
				&issue.id,
				run_id,
				attempt_number,
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
			.expect("phase goal event should record");
	}

	let first_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1"),
		retry_budget_base: 0,
	};
	let second_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: repo_root.to_path_buf(),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2"),
		retry_budget_base: 0,
	};
	let first = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&first_issue_run,
		&app_server_timeout,
	)
	.expect("first recovery should evaluate")
	.expect("first recovery should schedule continuation");
	let second = orchestrator::maybe_continue_after_phase_goal_recovery(
		&config,
		&workflow,
		&state_store,
		&second_issue_run,
		&app_server_timeout,
	)
	.expect("second recovery should evaluate");
	let events = state_store
		.list_private_execution_events_for_issue(TEST_SERVICE_ID, &issue.id)
		.expect("private phase goal events should load");
	let scheduled_events = events
		.iter()
		.filter(|event| event.event_type() == PHASE_GOAL_RECOVERY_EVENT_TYPE)
		.collect::<Vec<_>>();
	let blocked_event = events
		.iter()
		.find(|event| event.event_type() == PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE)
		.expect("second recovery should record blocked event");

	assert!(first.continuation_pending);
	assert!(second.is_none());
	assert_eq!(scheduled_events.len(), 1);
	assert_eq!(blocked_event.payload()["signal"], "continuation_budget_exhausted");
	assert_eq!(blocked_event.payload()["payload"]["priorRecoveryCount"], 1);
	assert_eq!(
		blocked_event.payload()["payload"]["automaticContinuationLimit"],
		orchestrator::PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT
	);
	assert!(blocked_event.payload()["payload"]["sourceErrorMessage"].as_str().is_some_and(
		|message| { message.contains("Timed out while waiting for app-server output.") }
	));
	assert_eq!(
		blocked_event.payload()["payload"]["sourceErrorClass"],
		"app_server_preflight_timeout"
	);
}
