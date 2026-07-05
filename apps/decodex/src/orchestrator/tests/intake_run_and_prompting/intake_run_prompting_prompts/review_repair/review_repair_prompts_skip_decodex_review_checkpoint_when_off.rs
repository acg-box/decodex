use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn review_repair_prompts_skip_decodex_review_checkpoint_when_off() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_max_turns(4);
	let config = tests::service_config_with_review_level(&config, ReviewLevel::Off);
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
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 2,
		run_id: String::from("pub-101-attempt-2-123"),
		retry_budget_base: 0,
	};
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		Some(pr_url),
	)
	.expect("review repair developer instructions should build");
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
		IssueDispatchMode::ReviewRepair,
		Some(pr_url),
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		assert!(prompt.contains("[codex].review = \"off\""));
		assert!(prompt.contains("do not call `issue_review_checkpoint`"));
		assert!(!prompt.contains("Follow the repo-native bounded review method"));
		assert!(!prompt.contains("only after the latest `issue_review_checkpoint`"));
		assert!(prompt.contains(ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME));
	}

	assert!(
		developer_instructions
			.contains("Call `issue_review_repair_complete` after the repaired head is pushed")
	);
	assert!(user_input.contains("required validation has passed"));
	assert!(continuation_input.contains("required validation has passed"));
	assert!(user_input.contains("validate each actionable claim against the codebase"));
	assert!(continuation_input.contains("Do not request GitHub Review from this run"));
}
