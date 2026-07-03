use crate::{
	orchestrator::{
		self, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME,
		IssueDispatchMode, IssueRunPlan, ReviewHandoffMarker, ReviewOrchestrationMarker,
		tests::{self, FakeTracker, intake_run_and_prompting},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn review_repair_prompts_ignore_newer_unrelated_branch_orchestration_records() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let current_handoff = ReviewHandoffMarker::new(
		"pub-101-attempt-4-123",
		4,
		"x/pubfi-pub-101",
		pr_url,
		"main",
		"x/pubfi-pub-101",
		"abc123",
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&current_handoff,
	);
	tests::seed_review_orchestration_marker(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewOrchestrationMarker::new(
			"pub-101-attempt-4-123",
			4,
			"x/pubfi-pub-101",
			pr_url,
			"abc123",
			"repair_required",
			None,
			None,
			None,
			0,
			3,
			None,
		),
	);

	let unrelated_handoff = ReviewHandoffMarker::new(
		"other-run",
		1,
		"x/pubfi-pub-101-next",
		"https://github.com/hack-ink/decodex/pull/88",
		"main",
		"x/pubfi-pub-101-next",
		"def456",
	);

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&unrelated_handoff,
	);
	tests::seed_review_orchestration_marker(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewOrchestrationMarker::new(
			"other-run",
			1,
			"x/pubfi-pub-101-next",
			"https://github.com/hack-ink/decodex/pull/88",
			"def456",
			"repair_required",
			None,
			None,
			None,
			0,
			4,
			None,
		),
	);

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: String::from("x/pubfi-pub-101"),
			issue_identifier: issue.identifier.clone(),
			path: worktree_path,
			reused_existing: true,
		},
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: 4,
		run_id: String::from("pub-101-attempt-4-123"),
		retry_budget_base: 0,
	};
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		Some(pr_url),
	)
	.expect("review repair developer instructions should build");

	assert!(!developer_instructions.contains("GitHub Review round 4"));
	assert!(!developer_instructions.contains("architectural or root-cause defect"));
}

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
	assert!(developer_instructions.contains("single-line `decodex/commit/1` JSON commit message"));
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
			.contains("either omit `head_sha` or pass the exact full current `HEAD` SHA")
	);
	assert!(continuation_input.contains(
		"If the issue is still in `In Review`, transition it once to `Done` with `issue_transition` before `issue_closeout_complete`"
	));
	assert!(continuation_input.contains("closeout"));

	for prompt in [&developer_instructions, &user_input, &continuation_input] {
		intake_run_and_prompting::assert_manual_attention_prompt_guidance(prompt, false);
	}
}
