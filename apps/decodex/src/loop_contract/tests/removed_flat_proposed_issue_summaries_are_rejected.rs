use crate::loop_contract::{DecisionContract, tests};

#[test]
fn removed_flat_proposed_issue_summaries_are_rejected() {
	let mut payload = serde_json::to_value(tests::latent_research_contract_fixture())
		.expect("fixture should encode");
	let readiness = payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Removed flat summary."]),
	);

	let error = serde_json::from_value::<DecisionContract>(payload)
		.expect_err("removed flat summaries must not deserialize");

	assert!(error.to_string().contains("proposed_issue_summaries"));
}
