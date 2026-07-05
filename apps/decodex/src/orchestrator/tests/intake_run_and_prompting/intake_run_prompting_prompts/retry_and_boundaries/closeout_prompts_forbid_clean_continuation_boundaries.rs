use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn closeout_prompts_forbid_clean_continuation_boundaries() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_max_turns(4);
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: String::from("PUB-101"),
			path: config.worktree_root().join("PUB-101"),
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Closeout,
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 0,
	};
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	)
	.expect("closeout developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::Closeout,
		Some(pr_url),
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		assert!(prompt.contains("short deterministic tail"));
		assert!(prompt.contains("Do not end the turn without"));
		assert!(!prompt.contains("you may end the turn without terminal finalization"));
	}
}
