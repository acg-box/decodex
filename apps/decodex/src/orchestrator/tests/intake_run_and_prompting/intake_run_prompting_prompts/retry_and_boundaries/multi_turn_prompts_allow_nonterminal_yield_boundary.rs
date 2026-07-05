use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn multi_turn_prompts_allow_nonterminal_yield_boundary() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_max_turns(4);
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: false,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("pub-101-attempt-1-123"),
		retry_budget_base: 0,
	};
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::Normal,
		None,
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	assert!(user_input.contains("you may end the turn without"));
	assert!(continuation_input.contains("you may end the turn without terminal finalization"));
	assert!(!user_input.contains("Do not end the turn"));
}
