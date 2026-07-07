use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan, ReviewLifecycleHandoffFixture,
		ReviewLifecycleTransitionFixture,
		tests::{self, FakeTracker},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

#[test]
fn review_repair_prompts_surface_architecture_check_on_fourth_external_round() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let pr_url = "https://github.com/hack-ink/decodex/pull/77";
	let review_handoff = ReviewLifecycleHandoffFixture::new(
		"pub-101-attempt-4-123",
		4,
		"x/pubfi-pub-101",
		pr_url,
		"main",
		"x/pubfi-pub-101",
		"abc123",
	);

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&review_handoff,
	);
	tests::seed_review_lifecycle_transition_fixture(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewLifecycleTransitionFixture::new(
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
