use crate::{
	loop_contract::DecisionContract,
	program_intake::{
		self, GoalIntakeRunRequest,
		tests::{test_support, test_support::FakeTracker},
	},
	state::StateStore,
};

#[test]
fn generated_issue_text_validation_rejects_private_program_identifiers() {
	let title_error = program_intake::validate_generated_issue_text(
		"Expose goal-decodex-contract-private",
		"## Objective\nUse normal public text.",
		&["goal-decodex-contract-private"],
	)
	.expect_err("title must reject private program id");

	assert!(
		title_error
			.to_string()
			.contains("generated issue title contains a private Program Intake identifier")
	);

	let description_error = program_intake::validate_generated_issue_text(
		"Use normal public text.",
		"## Objective\nExpose goal:contract:01-private-node.",
		&["goal:contract:01-private-node"],
	)
	.expect_err("description must reject private node id");

	assert!(
		description_error
			.to_string()
			.contains("generated issue description contains a private Program Intake identifier")
	);
}

#[test]
fn goal_intake_apply_rejects_generated_briefs_that_leak_autonomy_lineage_ids() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut payload = serde_json::to_value(test_support::accepted_goal_contract())
		.expect("accepted goal contract should serialize");

	payload["accepted_authority"]["constraints"]
		.as_array_mut()
		.expect("constraints should be an array")
		.push(serde_json::json!(
			"Do not expose autonomy_signal:test-signal in generated issue text."
		));

	let leaking_contract: DecisionContract =
		serde_json::from_value(payload).expect("leaking contract should deserialize");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), leaking_contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-852", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("private autonomy lineage ids must fail generated brief validation");

	assert!(
		error
			.to_string()
			.contains("generated issue description contains a private Program Intake identifier")
	);
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}
