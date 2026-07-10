use crate::{
	orchestrator::{
		self, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, IssueDispatchMode,
		IssueRunPlan,
		tests::{self, FakeTracker, intake_run_and_prompting},
	},
	state::StateStore,
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
	assert!(instructions.contains("Registered repo gate"));
	assert!(instructions.contains("`canonicalize_commands`: []"));
	assert!(instructions.contains("`verify_commands`: []"));
	assert!(instructions.contains("Do not substitute broader repo-documentation examples"));
	assert!(instructions.contains("Keep pre-edit discovery bounded"));
	assert!(instructions.contains("Do not browse upstream references"));
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
	assert!(instructions.contains(
		"Treat the active phase goal as the authoritative current step, not as a checklist ceremony"
	));
	assert!(instructions.contains("Decodex can run its repo gate, record validation evidence"));
	assert!(!instructions.contains("Do not use `issue_progress_checkpoint`, final chat text"));
	assert!(instructions.contains("treat `issue_progress_checkpoint` as terminal completion"));
	assert!(!instructions.contains("you may end the turn without"));
	assert!(!instructions.contains("WORKFLOW.md\n"));
}

#[test]
fn developer_instructions_render_registered_repo_gate_commands() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Follow the repository policy.\n", 1)
			.replace("canonicalize_commands = []", r#"canonicalize_commands = ["cargo make fmt"]"#)
			.replace("verify_commands = []", r#"verify_commands = ["cargo make test"]"#),
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

	assert!(instructions.contains("`canonicalize_commands`: `cargo make fmt`"));
	assert!(instructions.contains("`verify_commands`: `cargo make test`"));
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
fn normal_prompts_delegate_decodex_review_to_runtime_gate() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let surfaces = intake_run_and_prompting::build_normal_prompt_surfaces(&config, &workflow);

	for prompt in surfaces.all() {
		intake_run_and_prompting::assert_runtime_owned_review_prompt_guidance(prompt);
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
	assert!(developer_instructions.contains("single-line `decodex/commit/2` JSON commit message"));
}
