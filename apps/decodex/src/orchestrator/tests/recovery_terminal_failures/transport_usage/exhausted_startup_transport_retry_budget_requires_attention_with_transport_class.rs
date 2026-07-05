use std::fs;

use color_eyre::Report;

use crate::{
	agent::AppServerTransportFailure,
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{
			FakeTracker, {self},
		},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn exhausted_startup_transport_retry_budget_requires_attention_with_transport_class() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let tracker = FakeTracker::new(vec![]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Progress", &[]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
	};
	let error = Report::new(AppServerTransportFailure::with_phase(
		String::from("App-server stdout disconnected unexpectedly."),
		"thread/start",
		true,
	));

	fs::create_dir_all(&issue_run.worktree.path).expect("worktree path should exist");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "failed")
		.expect("run attempt should record");

	orchestrator::handle_failure(&tracker, &config, &workflow, &state_store, &issue_run, &error)
		.expect("exhausted startup transport failure should require attention");

	assert_eq!(
		tracker.state_updates.borrow().last(),
		Some(&(issue.id.clone(), String::from("state-todo")))
	);
	assert!(tracker.comments.borrow().iter().any(|comment| {
		comment.contains("decodex run failed and needs attention")
			&& comment.contains("app_server_transport_disconnected")
			&& comment.contains("failure during `thread/start` manually")
	}));
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and will retry"))
	);
}
