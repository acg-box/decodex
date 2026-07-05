use crate::{
	program_intake::{
		self, GoalIntakeRunRequest,
		tests::{test_support, test_support::FakeTracker},
	},
	state::StateStore,
};

#[test]
fn goal_intake_refuses_latent_or_missing_decision_authority() {
	let store = StateStore::open_in_memory().expect("store should open");
	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-852", "Todo")]);
	let latent = test_support::latent_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), latent)
		.expect("latent contract should persist");

	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let latent_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("latent contract must not materialize");

	assert!(latent_error.to_string().contains("requires accepted execution authority"));

	let mut needs_decision = test_support::latent_goal_contract();

	needs_decision
		.require_human_decision("Choose the public issue split before apply.")
		.expect("contract should record missing decision");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), needs_decision)
		.expect("needs-decision contract should persist");

	let missing_decision_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("missing decision must stop apply");

	assert!(missing_decision_error.to_string().contains("needs_human_decision"));
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}
