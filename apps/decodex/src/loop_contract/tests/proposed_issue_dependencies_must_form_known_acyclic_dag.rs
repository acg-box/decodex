use crate::loop_contract::{DecisionContract, tests};

#[test]
fn proposed_issue_dependencies_must_form_known_acyclic_dag() {
	let mut missing_dependency = serde_json::to_value(tests::latent_research_contract_fixture())
		.expect("fixture should encode");

	missing_dependency["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!(["missing-node"]);

	let error = serde_json::from_value::<DecisionContract>(missing_dependency)
		.expect("contract should deserialize")
		.validate()
		.expect_err("unknown dependency must be rejected");

	assert!(error.to_string().contains("depends on unknown issue `missing-node`"));

	let self_key =
		tests::latent_research_contract_fixture().execution_readiness().proposed_issues()[0]
			.key()
			.to_owned();
	let mut self_dependency = serde_json::to_value(tests::latent_research_contract_fixture())
		.expect("fixture should encode");

	self_dependency["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!([self_key]);

	let error = serde_json::from_value::<DecisionContract>(self_dependency)
		.expect("contract should deserialize")
		.validate()
		.expect_err("self dependency must be rejected");

	assert!(error.to_string().contains("must not depend on itself"));

	let mut cyclic = serde_json::to_value(tests::latent_research_contract_fixture())
		.expect("fixture should encode");
	let first_key = cyclic["execution_readiness"]["proposed_issues"][0]["key"]
		.as_str()
		.expect("first key")
		.to_owned();
	let mut second = cyclic["execution_readiness"]["proposed_issues"][0].clone();

	second["key"] = serde_json::json!("research-x-validation");
	second["title"] = serde_json::json!("Validate the Decision Contract graph.");
	second["objective"] = serde_json::json!("Validate the Decision Contract graph.");
	second["dependencies"] = serde_json::json!([first_key]);
	cyclic["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!(["research-x-validation"]);

	cyclic["execution_readiness"]["proposed_issues"]
		.as_array_mut()
		.expect("proposed issues should be array")
		.push(second);

	let error = serde_json::from_value::<DecisionContract>(cyclic)
		.expect("contract should deserialize")
		.validate()
		.expect_err("dependency cycle must be rejected");

	assert!(error.to_string().contains("dependency cycle includes"));
}
