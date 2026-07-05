use crate::{
	program_intake::{
		self, GoalIntakeRunRequest,
		tests::{test_support, test_support::FakeTracker},
	},
	state::StateStore,
	tracker::IssueTracker,
};

#[test]
fn goal_intake_apply_creates_updates_and_persists_links() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = test_support::accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
		.expect("existing generated link should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([test_support::issue("XY-852", "Todo"), test_support::issue("XY-G1", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("apply should materialize issues and program");

	test_support::assert_goal_intake_apply_report(&report, &tracker);
	test_support::assert_goal_intake_runtime_links(&store, &report);

	let updated = tracker
		.get_issue_by_identifier("XY-G1")
		.expect("issue lookup should work")
		.expect("updated issue should exist");

	test_support::assert_goal_issue_brief_is_public(&updated.description, &report);
}

#[test]
fn goal_intake_apply_persists_links_after_each_successful_issue_mutation() {
	let store = StateStore::open_in_memory().expect("store should open");
	let contract = test_support::accepted_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([test_support::issue("XY-852", "Todo")])
		.with_create_failure_after_successes(1);
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
	.expect_err("second issue create should fail");

	assert!(error.to_string().contains("injected create failure"));
	assert_eq!(tracker.created_issue_count(), 1);

	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1")]
	);
	assert_eq!(linked_contract.contract().links().generated_issue_ids().len(), 1);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 1);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}
