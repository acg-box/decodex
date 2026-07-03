use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME,
		ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, IssueDispatchMode, IssueRunPlan, ReviewHandoffMarker,
		ReviewOrchestrationMarker,
		tests::{
			self, FakeTracker, TEST_SERVICE_ID, intake_run_and_prompting, intake_workflow_reload,
		},
	},
	state::StateStore,
	workflow::WorkflowDocument,
	worktree::WorktreeSpec,
};

#[test]
fn developer_instructions_trim_workflow_body_and_preserve_required_guidance() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue,
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
	let instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	)
	.expect("developer instructions should build");

	assert!(instructions.contains("Workflow policy\nFollow the repository policy.\n"));
	assert!(instructions.contains("Keep pre-edit discovery bounded"));
	assert!(instructions.contains("Do not browse upstream references"));
	assert!(instructions.contains("Docs impact contract"));
	assert!(instructions.contains(
		"classify docs impact as `none`, `update_required`, `research_required`, or `drift_required`"
	));
	assert!(
		instructions
			.contains("record it in a current-HEAD `issue_progress_checkpoint` as `docs_impact`")
	);
	assert!(instructions.contains("Tracker tool contract"));
	assert!(instructions.contains("Linear tracker text is public/team-visible"));
	assert!(instructions.contains("You own issue-scoped tracker writes for `PUB-101`."));
	assert!(instructions.contains("Decodex already records the run-start Linear ledger"));
	assert!(!instructions.contains("started work on run"));
	assert!(
		instructions.contains("Do not speculate about capabilities you did not directly verify.")
	);
	assert!(instructions.contains(ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME));
	assert!(instructions.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	assert!(instructions.contains(ISSUE_REVIEW_HANDOFF_TOOL_NAME));
	assert!(instructions.contains(ISSUE_TERMINAL_FINALIZE_TOOL_NAME));
	assert!(instructions.contains("Phase goal runtime contract"));
	assert!(
		instructions.contains("Treat the active phase goal as the authoritative current contract")
	);
	assert!(instructions.contains(
		"explicitly complete the active phase goal with the Codex goal completion mechanism"
	));
	assert!(
		instructions.contains(
			"Do not use `issue_progress_checkpoint`, final chat text, or an \"await next phase\" statement as a substitute"
		)
	);
	assert!(instructions.contains("treat `issue_progress_checkpoint` as terminal completion"));
	assert!(!instructions.contains("you may end the turn without"));
	assert!(!instructions.contains("WORKFLOW.md\n"));
}

#[test]
fn normal_prompts_record_manual_attention_label_intent_before_label_application() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let surfaces = intake_run_and_prompting::build_normal_prompt_surfaces(&config, &workflow);

	for prompt in surfaces.all() {
		intake_run_and_prompting::assert_manual_attention_prompt_guidance(prompt, true);
	}
}

#[test]
fn normal_prompts_require_review_signal_routes_before_repair() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let surfaces = intake_run_and_prompting::build_normal_prompt_surfaces(&config, &workflow);

	for prompt in surfaces.all() {
		intake_run_and_prompting::assert_review_route_prompt_guidance(prompt);

		assert!(prompt.contains(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME));
	}
}

#[test]
fn review_pull_request_title_normalizes_issue_prefix() {
	for title in [
		"Ensure Decodex-created PR titles include issue authority prefix",
		"xy-381: Ensure Decodex-created PR titles include issue authority prefix",
	] {
		let mut issue = tests::sample_issue("Todo", &[]);

		issue.identifier = String::from("XY-381");
		issue.title = String::from(title);

		assert_eq!(
			orchestrator::review_pull_request_title(&issue),
			"XY-381: Ensure Decodex-created PR titles include issue authority prefix"
		);
	}
}

#[test]
fn normal_prompts_require_issue_prefixed_pull_request_title() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.identifier = String::from("XY-381");
	issue.title = String::from("Ensure Decodex-created PR titles include issue authority prefix");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		worktree: WorktreeSpec {
			branch_name: String::from("y/decodex-xy-381"),
			issue_identifier: String::from("XY-381"),
			path: config.worktree_root().join("XY-381"),
			reused_existing: false,
		},
		retry_project_slug: String::from("decodex"),
		dispatch_mode: IssueDispatchMode::Normal,
		attempt_number: 1,
		run_id: String::from("xy-381-attempt-1-123"),
		retry_budget_base: 0,
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
	let continuation_input = orchestrator::build_continuation_user_input(
		&issue,
		&workflow,
		IssueDispatchMode::Normal,
		None,
		workflow.frontmatter().tracker().success_state(),
		config.codex().review_level(),
	);
	let expected_title = "XY-381: Ensure Decodex-created PR titles include issue authority prefix";
	let create_or_update_instruction =
		format!("create or update a non-draft PR titled `{expected_title}`");

	assert!(developer_instructions.contains(&create_or_update_instruction));
	assert!(user_input.contains(&create_or_update_instruction));
	assert!(
		continuation_input
			.contains(&format!("ensure the non-draft PR title is `{expected_title}`"))
	);
	assert!(developer_instructions.contains("single-line `decodex/commit/1` JSON commit message"));
}

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

#[test]
fn single_turn_prompts_do_not_allow_nonterminal_yield_boundary() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue,
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
	let developer_instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	)
	.expect("developer instructions should build");
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&tests::sample_issue("Todo", &[]),
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	);

	assert!(!developer_instructions.contains("you may end the turn without"));
	assert!(!user_input.contains("you may end the turn without"));
}

#[test]
fn prompts_handle_machine_only_and_text_fenced_tracker_descriptions() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let cases: &[(&str, &str, &[&str])] = &[
		(
			"single json fence",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\",\n  \"id\": \"ptr-1\"\n}\n```",
			&["\"schema\": \"opaque-pointer/1\""],
		),
		(
			"multiple json fences",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n```\n\n```json\n{\n  \"schema\": \"opaque-pointer/2\"\n}\n```",
			&["\"schema\": \"opaque-pointer/1\"", "\"schema\": \"opaque-pointer/2\""],
		),
		(
			"four backtick json fence",
			"````json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n````",
			&["\"schema\": \"opaque-pointer/1\""],
		),
		(
			"tilde json fence",
			"~~~json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n~~~",
			&["\"schema\": \"opaque-pointer/1\""],
		),
	];

	for (case_name, description, forbidden_fragments) in cases {
		let mut issue = tests::sample_issue("Todo", &[]);

		issue.description = (*description).to_owned();

		let tracker = FakeTracker::new(vec![issue.clone()]);
		let issue_run = intake_run_and_prompting::normal_prompt_issue_run(&config, issue.clone());
		let user_input = orchestrator::build_user_input(
			&tracker,
			&config,
			&issue,
			&workflow,
			&issue_run,
			&StateStore::open_in_memory().expect("state store should open"),
			None,
		);

		assert!(
			user_input.contains("machine-only tracker description omitted"),
			"{case_name} should be redacted"
		);

		for forbidden in *forbidden_fragments {
			assert!(!user_input.contains(forbidden), "{case_name} leaked {forbidden}");
		}
	}

	let mut issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	issue.description =
		String::from("```text\nImplement the retained lane repair and keep scope tight.\n```");

	let issue_run = intake_run_and_prompting::normal_prompt_issue_run(&config, issue.clone());
	let user_input = orchestrator::build_user_input(
		&tracker,
		&config,
		&issue,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	);

	assert!(!user_input.contains("machine-only tracker description omitted"));
	assert!(user_input.contains("Implement the retained lane repair and keep scope tight."));
}

#[test]
fn developer_instructions_match_trimmed_prompt_shape() {
	let read_first_files = [
		("docs/index.md", "Use the documentation index.\n"),
		("docs/runbook/index.md", "Use the runbook index.\n"),
	];
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_read_first(
		&read_first_files,
		"This workflow body should be appended.\n",
	);
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let issue_run = IssueRunPlan {
		issue,
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
	let instructions = orchestrator::build_developer_instructions(
		&tracker,
		&config,
		&workflow,
		&issue_run,
		&StateStore::open_in_memory().expect("state store should open"),
		None,
	)
	.expect("developer instructions should build");

	assert_eq!(
		instructions,
		intake_workflow_reload::expected_developer_instructions(
			&read_first_files,
			&workflow,
			&issue_run
		)
	);
}
