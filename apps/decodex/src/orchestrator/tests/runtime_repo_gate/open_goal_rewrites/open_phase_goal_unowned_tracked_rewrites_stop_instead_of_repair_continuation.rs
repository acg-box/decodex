use std::fs;

use color_eyre::Report;

use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, RepoGateFailure, StateStore, tests,
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
	support::record_validation_evidence_progress_checkpoint(&config, &state_store, &issue_run, &[]);

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

	assert_eq!(
		repo_gate_failure.error_class(),
		"repo_gate_lane_external_tracked_rewrite"
	);
	assert_eq!(
		repo_gate_failure.disposition(),
		orchestrator::RepoGateFailureDisposition::NeedsHumanAttention
	);
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_fail"
			&& event.payload()["payload"]["disposition"] == "needs_human_attention"
			&& event.payload()["payload"]["trackedRewrites"]["classification"]
				== "lane_external_tracked_rewrite"
			&& event.payload()["payload"]["trackedRewrites"]["decision"]
				== "require_scoped_authority"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == false
			&& event.payload()["payload"]["trackedRewrites"]["rewriteSetHash"]
				.as_str()
				.is_some_and(|hash| hash.len() == 64)
			&& event.payload()["payload"]["trackedRewrites"]["files"]
				.as_array()
				.is_some_and(|files| files.iter().any(|file| file.as_str() == Some("other.txt")))
	}));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_next"));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_recovery"));
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["repo_gate_error_class"]
				== "repo_gate_lane_external_tracked_rewrite"
			&& event.payload()["reason"]
				== "repo-gate lane-external tracked rewrite requires project cleanup or explicit scoped-gate authority"
	}));
}
