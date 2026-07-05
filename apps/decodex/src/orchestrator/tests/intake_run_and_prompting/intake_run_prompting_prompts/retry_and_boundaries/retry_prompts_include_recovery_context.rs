use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn retry_prompts_include_recovery_context() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Progress", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("In Progress"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Retry,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 1,
	};
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&state_store,
		None,
	);

	for prompt in [&developer_instructions, &user_input] {
		assert!(prompt.contains("Recovery context"));
		assert!(prompt.contains("Treat the current worktree"));
		assert!(prompt.contains("Do not assume in-memory model output or tool results survived"));
	}
}
