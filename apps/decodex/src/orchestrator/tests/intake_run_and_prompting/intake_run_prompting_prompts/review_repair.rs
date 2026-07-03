use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, IssueDispatchMode, IssueRunPlan,
		ReviewHandoffMarker, ReviewOrchestrationMarker,
		tests::{self, FakeTracker, intake_run_and_prompting},
	},
	state::StateStore,
	workflow::WorkflowDocument,
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

#[test]
fn review_repair_continuation_prompt_uses_configured_success_state() {
	let workflow = WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "Ready For QA"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 4
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Custom workflow.
"#,
	)
	.expect("workflow should parse");
	let issue = tests::sample_issue("Ready For QA", &[]);
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::ReviewRepair,
		Some("https://github.com/hack-ink/decodex/pull/77"),
		workflow.frontmatter().tracker().success_state(),
		ReviewLevel::Standard,
	);

	assert!(continuation_input.contains("Ready For QA"));
	assert!(!continuation_input.contains("do not move the issue out of `In Review`"));
}

#[test]
fn review_repair_prompts_surface_architecture_check_on_fourth_external_round() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let review_handoff = ReviewHandoffMarker::new(
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
		&review_handoff,
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
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&state_store,
		Some(pr_url),
	);

	assert!(developer_instructions.contains("GitHub Review round 4"));
	assert!(developer_instructions.contains("architectural or root-cause defect"));
	assert!(developer_instructions.contains("reset the GitHub Review round budget"));
	assert!(user_input.contains("GitHub Review round 4"));
	assert!(user_input.contains("Do not request GitHub Review yourself"));
}
