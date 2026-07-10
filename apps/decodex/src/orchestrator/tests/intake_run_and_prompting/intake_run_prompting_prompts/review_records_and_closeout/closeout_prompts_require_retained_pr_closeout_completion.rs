use crate::{
	orchestrator::{
		self, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME,
		IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker, intake_run_and_prompting},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn closeout_prompts_require_retained_pr_closeout_completion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: config.worktree_root().join(&issue.identifier),
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

	assert!(developer_instructions.contains(ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME));
	assert!(developer_instructions.contains(ISSUE_TRANSITION_TOOL_NAME));
	assert!(developer_instructions.contains("Merge is already authoritative"));
	assert!(developer_instructions.contains("Do not land, merge, or request review"));
	assert!(developer_instructions.contains("single-line `decodex/commit/2` JSON commit message"));
	assert!(developer_instructions.contains("do not call `issue_review_handoff`"));
	assert!(developer_instructions.contains("may already be in `Done`"));
	assert!(developer_instructions.contains(
		"either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA"
	));
	assert!(developer_instructions.contains(
		"If the issue is still in `In Review`, transition it once to `Done` with `issue_transition` before `issue_closeout_complete`"
	));
	assert!(user_input.contains("merged PR lineage"));
	assert!(user_input.contains("Merge is already authoritative"));
	assert!(user_input.contains("Do not land, merge, or request review"));
	assert!(user_input.contains("may already be in `Done`"));
	assert!(user_input.contains(
		"either omit `head_sha` and let `decodex` record the exact current lane HEAD automatically, or pass the exact full current `HEAD` SHA"
	));
	assert!(user_input.contains(
		"If the issue is still in `In Review`, move it once to `Done` with `issue_transition` before `issue_closeout_complete`"
	));
	assert!(user_input.contains("closeout"));
	assert!(continuation_input.contains("merged PR lineage"));
	assert!(continuation_input.contains("Merge is already authoritative"));
	assert!(continuation_input.contains("Do not land, merge, or request review"));
	assert!(continuation_input.contains("may already be in `Done`"));
	assert!(
		continuation_input
			.contains("omit `head_sha` to capture the current lane HEAD automatically or pass the exact full current `HEAD` SHA")
	);
	assert!(continuation_input.contains(
		"If the issue is still in `In Review`, transition it once to `Done` with `issue_transition` before `issue_closeout_complete`"
	));
	assert!(continuation_input.contains("closeout"));

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		intake_run_and_prompting::assert_manual_attention_prompt_guidance(prompt, false);
	}
}
