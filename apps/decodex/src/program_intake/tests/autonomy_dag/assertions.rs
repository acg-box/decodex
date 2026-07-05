use crate::{
	loop_contract::DecisionContract,
	program_intake::{GoalIntakeIssueAction, GoalIntakeReport, tests::test_support::FakeTracker},
	state::StateStore,
};

pub(crate) fn assert_autonomy_dag_goal_intake_result(
	store: &StateStore,
	tracker: &FakeTracker,
	contract: &DecisionContract,
	report: &GoalIntakeReport,
) {
	assert!(report.applied);
	assert!(report.persisted);
	assert_eq!(tracker.created_issue_count(), 2);
	assert_eq!(tracker.updated_issue_count(), 0);
	assert_eq!(report.issues.len(), 2);
	assert_eq!(report.issues[0].action, GoalIntakeIssueAction::Created);
	assert_eq!(report.issues[0].dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(report.issues[1].action, GoalIntakeIssueAction::Created);
	assert_eq!(report.issues[1].dispatch_action, None);
	assert!(
		report.issues[1]
			.reasons
			.iter()
			.any(|reason| reason.contains("has not reached a required terminal state")),
		"dependent node should wait for the first generated issue to complete"
	);

	let programs = store.list_execution_programs("decodex").expect("programs should list");

	assert_eq!(programs.len(), 1);
	assert_eq!(programs[0].program().source_contract_id(), Some(contract.contract_id()));
	assert_eq!(programs[0].program().nodes().len(), 2);

	let program_json = serde_json::to_value(programs[0].program())
		.expect("program should serialize for dependency inspection");

	assert_eq!(
		program_json["nodes"][1]["dependencies"][0]["dependency_id"],
		program_json["nodes"][0]["node_id"]
	);

	let linked_contract = store
		.decision_contract("decodex", contract.contract_id())
		.expect("contract readback should work")
		.expect("linked contract should exist");

	assert_eq!(linked_contract.contract().links().generated_issue_identifiers().len(), 2);

	let intake_plans = store.list_program_intake_plans("decodex").expect("intake plans");

	assert_eq!(intake_plans.len(), 1);
	assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
	assert_eq!(intake_plans[0].source_contract_id(), Some(contract.contract_id()));
}
