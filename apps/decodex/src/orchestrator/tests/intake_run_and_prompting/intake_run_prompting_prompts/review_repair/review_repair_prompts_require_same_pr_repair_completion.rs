use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker, intake_run_and_prompting},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn review_repair_prompts_require_same_pr_repair_completion() {
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

	intake_run_and_prompting::assert_review_repair_developer_prompt(&developer_instructions);
	intake_run_and_prompting::assert_review_repair_user_prompt(&user_input, pr_url);
	intake_run_and_prompting::assert_review_repair_continuation_prompt(&continuation_input);

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		intake_run_and_prompting::assert_manual_attention_prompt_guidance(prompt, false);
	}

	intake_run_and_prompting::assert_prompt_orders_thread_replies_after_push(
		&developer_instructions,
		"push the repaired head.",
	);
	intake_run_and_prompting::assert_prompt_orders_thread_replies_after_push(
		&user_input,
		"Commit the repair and push the same branch.",
	);
	intake_run_and_prompting::assert_prompt_orders_thread_replies_after_push(
		&continuation_input,
		"If the repaired head is ready, push it.",
	);
}
