mod assertions;
mod contract;
mod fixtures;
mod proposal;

use crate::{
	program_intake::{
		self, GoalIntakeRunRequest,
		tests::{test_support, test_support::FakeTracker},
	},
	state::StateStore,
};

#[test]
fn autonomy_proposal_issue_dag_materializes_through_goal_intake_in_isolated_store() {
	let store = StateStore::open_in_memory().expect("isolated store should open");
	let contract = contract::promoted_autonomy_dag_contract(&store);
	let tracker = FakeTracker::default().with_issues([test_support::issue("XY-2000", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: contract.contract_id(),
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("isolated goal intake should materialize the proposal issue DAG");

	assertions::assert_autonomy_dag_goal_intake_result(&store, &tracker, &contract, &report);
}
