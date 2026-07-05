use crate::{
	program_intake::{
		self, GoalIntakeRunRequest,
		tests::{test_support, test_support::FakeTracker},
	},
	state::StateStore,
};

#[test]
fn goal_intake_apply_preserves_later_existing_links_after_update_failure() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = test_support::accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(
			["id-XY-G1", "id-XY-G2"],
			["XY-G1", "XY-G2"],
			["old-node-1", "old-node-2"],
		)
		.expect("existing generated links should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([
			test_support::issue("XY-852", "Todo"),
			test_support::issue("XY-G1", "Todo"),
			test_support::issue("XY-G2", "Todo"),
		])
		.with_update_failure_after_successes(1);
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
	.expect_err("second issue update should fail");

	assert!(error.to_string().contains("injected update failure"));
	assert_eq!(tracker.updated_issue_count(), 1);

	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1"), String::from("XY-G2")]
	);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 2);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids()[1], "old-node-2");
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_fails_closed_when_existing_generated_link_is_missing() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = test_support::accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
		.expect("existing generated link should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
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
	.expect_err("missing generated issue link should block apply");

	assert!(error.to_string().contains("Generated issue link `XY-G1`"));
	assert_eq!(tracker.created_issue_count(), 0);
	assert_eq!(tracker.updated_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}
