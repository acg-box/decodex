use crate::{
	orchestrator::{
		self, IssueDispatchMode, IssueRunPlan,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::StateStore,
	worktree::WorktreeSpec,
};

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
