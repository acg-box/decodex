use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, AppServerCapabilityPreflightFailure, IssueDispatchMode, IssueRunPlan,
		PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE, PHASE_GOAL_RECOVERY_EVENT_TYPE, StateStore, tests,
		tests::TEST_SERVICE_ID,
	},
	tracker,
	worktree::WorktreeSpec,
};

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
