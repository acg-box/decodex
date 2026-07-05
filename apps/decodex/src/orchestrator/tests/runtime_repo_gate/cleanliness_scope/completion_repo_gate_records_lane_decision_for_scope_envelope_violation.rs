use std::fs;

use crate::{
	orchestrator::{
		self, PhaseGoalKind, RepoGateFailure, StateStore, tests,
		tests::{TEST_SERVICE_ID, runtime_repo_gate::support},
	},
	tracker,
};

#[test]
fn completion_repo_gate_records_lane_decision_for_scope_envelope_violation() {
	let failing_verify = "printf 'rewritten\\n' > outside.txt; exit 1";
	let workflow_markdown =
		tests::sample_workflow_markdown("pubfi", &[], "Completion gate policy.\n", 1).replace(
			"verify_commands = []",
			&format!(
				"verify_commands = [{}]",
				serde_json::to_string(failing_verify).expect("command should serialize")
			),
		);
	let (_temp_dir, config, workflow) =
		tests::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let issue = tests::sample_issue(
		"In Progress",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_run = support::phase_goal_repo_gate_issue_run(&config, &issue);

	tests::commit_worktree_change(config.repo_root(), "owned.txt", "before\n", "add owned file");
	tests::commit_worktree_change(
		config.repo_root(),
		"outside.txt",
		"before\n",
		"add outside file",
	);
	fs::write(config.repo_root().join("owned.txt"), "implementation\n")
		.expect("pre-gate implementation diff should write");

	let error = orchestrator::run_completion_repo_gate(
		&config,
		&workflow,
		&state_store,
		&issue_run,
		PhaseGoalKind::HandoffEvidence,
	)
	.expect_err("completion repo-gate scope violation should stop");
	let repo_gate_failure = error
		.downcast_ref::<RepoGateFailure>()
		.expect("scope envelope violation should preserve repo-gate classification");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, &issue_run.run_id, 1)
		.expect("private lane decision events should load");

	assert_eq!(repo_gate_failure.error_class(), "repo_gate_scope_envelope_violation");

	let decision = repo_gate_failure
		.tracked_rewrite_decision()
		.expect("scope envelope violation should retain rewrite decision");
	let decision_json = decision.to_json();

	assert_eq!(decision_json["sourceErrorClass"], "repo_gate_verify_failed");
	assert_eq!(decision_json["sourceRepoGateFailure"]["stage"], "verify");
	assert!(events.iter().any(|event| {
		event.event_type() == "lane_decision"
			&& event.payload()["next_action"] == "needs_attention"
			&& event.payload()["repo_gate_disposition"] == "needs_human_attention"
			&& event.payload()["scope_envelope_violation"] == true
	}));
}
