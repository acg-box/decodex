use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, StateStore,
		tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
	worktree::WorktreeSpec,
};

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
