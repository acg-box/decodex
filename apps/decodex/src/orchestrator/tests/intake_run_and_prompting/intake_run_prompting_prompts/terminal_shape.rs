use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker, intake_run_and_prompting, intake_workflow_reload},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

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
		("CONTRIBUTING.md", "Use the contribution guide.\n"),
		("Makefile.toml", "Use the runbook index.\n"),
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
