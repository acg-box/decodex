use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker, TEST_SERVICE_ID, intake_run_and_prompting},
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

#[test]
fn architecture_recovery_prompt_uses_only_latest_active_recovery_start() {
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
		attempt_number: 3,
		run_id: String::from("pub-101-attempt-3-123"),
		retry_budget_base: 2,
	};
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2-123",
			2,
			"architecture_recovery_started",
			serde_json::json!({
				"schema": "decodex.architecture_recovery_started/1",
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": "review_churn",
				"recovery_budget": {
					"attempt": 1,
					"max_attempts": 1,
				},
			}),
		)
		.expect("architecture recovery start event should record");

	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");

	assert!(developer_instructions.contains("Architecture recovery context"));
	assert!(developer_instructions.contains("guardrail `review_churn`"));

	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			"pub-101-attempt-2-123",
			2,
			"architecture_recovery_terminal",
			serde_json::json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"reason_code": "architecture_recovery_exhausted",
				"guardrail_reason": "review_churn",
				"recovery_budget": {
					"attempt": 2,
					"max_attempts": 1,
				},
			}),
		)
		.expect("architecture recovery terminal event should record");

	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&state_store,
		None,
	)
	.expect("developer instructions should build");

	assert!(!developer_instructions.contains("Architecture recovery context"));
	assert!(!developer_instructions.contains("guardrail `review_churn`"));
}

#[test]
fn normal_prompts_respect_non_standard_review_levels() {
	for (mode, expected, forbidden_checkpoint) in [
		(ReviewLevel::Off, "[codex].review = \"off\"", None),
		(
			ReviewLevel::Basic,
			"Self Check: Review your work repeatedly and fix any logic bugs until no new issues are found.",
			Some(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME),
		),
	] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let config = tests::service_config_with_review_level(&config, mode);
		let prompts = intake_run_and_prompting::build_normal_prompt_surfaces(&config, &workflow);

		for prompt in prompts.all() {
			assert!(prompt.contains(expected), "{mode:?} prompt should contain `{expected}`");
			assert!(!prompt.contains("Follow the repo-native bounded review method"));

			if let Some(forbidden_checkpoint) = forbidden_checkpoint {
				assert!(!prompt.contains(forbidden_checkpoint));
			}

			assert!(!prompt.contains("only after the latest `issue_review_checkpoint`"));
		}

		assert!(
			prompts
				.developer_instructions
				.contains("Call `issue_review_handoff` after the branch is pushed")
		);
		assert!(prompts.user_input.contains("required validation has passed"));
		assert!(prompts.continuation_input.contains("after required validation has passed"));
	}
}

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
