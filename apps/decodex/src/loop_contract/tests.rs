use crate::loop_contract::{
	DecisionContract, DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind,
};

fn latent_research_contract_fixture() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("research X latent contract fixture should deserialize")
}

fn sample_promotion() -> DecisionPromotion {
	DecisionPromotion {
		accepted_by: String::from("operator"),
		accepted_by_kind: DecisionPromotionActorKind::User,
		accepted_at: String::from("2026-06-09T10:00:00Z"),
		acceptance_source: String::from("conversation"),
		promotion_reason: Some(String::from("User asked to push this forward.")),
	}
}

#[test]
fn latent_research_contract_fixture_serializes_with_expected_boundary() {
	let contract = latent_research_contract_fixture();

	contract.validate().expect("latent contract should validate");

	assert_eq!(contract.contract_id(), "research-x-loop-contract");
	assert_eq!(contract.status(), DecisionContractStatus::DraftLatent);
	assert_eq!(contract.source_intent().summary(), "Research X and shape follow-up work.");
	assert_eq!(contract.accepted_authority().accepted_objectives().len(), 2);
	assert!(contract.execution_readiness().ready_for_issue_shaping());
	assert_eq!(contract.evidence_boundary.private_evidence_refs().len(), 1);
	assert_eq!(contract.evidence_boundary.public_projection_refs().len(), 1);
	assert!(contract.promotion().is_none());
}

#[test]
fn promotion_records_acceptance_metadata_and_blocks_double_promotion() {
	let mut contract = latent_research_contract_fixture();

	contract.promote(sample_promotion()).expect("latent contract should promote");

	assert_eq!(contract.status(), DecisionContractStatus::AcceptedPromoted);
	assert_eq!(
		contract.promotion().expect("promotion should exist").accepted_at(),
		"2026-06-09T10:00:00Z"
	);
	assert!(
		contract.promote(contract.promotion().expect("promotion should exist").clone()).is_err()
	);
}

#[test]
fn rejected_contract_cannot_be_promoted() {
	let mut contract = latent_research_contract_fixture();

	contract
		.reject_or_supersede(Some(String::from("research-x-replacement")))
		.expect("contract should reject");

	assert_eq!(contract.status(), DecisionContractStatus::RejectedSuperseded);
	assert_eq!(contract.links().superseded_by_contract_id(), Some("research-x-replacement"));
	assert!(
		contract
			.promote(DecisionPromotion { promotion_reason: None, ..sample_promotion() })
			.is_err()
	);
}

#[test]
fn accepted_contracts_require_readiness_without_missing_decisions() {
	let mut contract = latent_research_contract_fixture();

	contract.execution_readiness.ready_for_issue_shaping = false;

	let before_failed_promotion = contract.clone();

	assert!(contract.promote(sample_promotion()).is_err());
	assert_eq!(contract, before_failed_promotion, "failed promotion must not mutate the contract");

	let mut contract = latent_research_contract_fixture();

	contract
		.execution_readiness
		.missing_decisions
		.push(String::from("Choose the first generated issue."));

	let before_failed_promotion = contract.clone();

	assert!(contract.promote(sample_promotion()).is_err());
	assert_eq!(contract, before_failed_promotion, "failed promotion must not mutate the contract");

	let mut contract = latent_research_contract_fixture();

	contract.execution_readiness.proposed_issues.clear();

	let before_failed_promotion = contract.clone();

	assert!(contract.promote(sample_promotion()).is_err());
	assert_eq!(contract, before_failed_promotion, "failed promotion must not mutate the contract");
}

#[test]
fn proposed_issue_dependencies_must_form_known_acyclic_dag() {
	let mut missing_dependency =
		serde_json::to_value(latent_research_contract_fixture()).expect("fixture should encode");

	missing_dependency["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!(["missing-node"]);

	let error = serde_json::from_value::<DecisionContract>(missing_dependency)
		.expect("contract should deserialize")
		.validate()
		.expect_err("unknown dependency must be rejected");

	assert!(error.to_string().contains("depends on unknown issue `missing-node`"));

	let self_key = latent_research_contract_fixture().execution_readiness().proposed_issues()[0]
		.key()
		.to_owned();
	let mut self_dependency =
		serde_json::to_value(latent_research_contract_fixture()).expect("fixture should encode");

	self_dependency["execution_readiness"]["proposed_issues"][0]["dependencies"] =
		serde_json::json!([self_key]);

	let error = serde_json::from_value::<DecisionContract>(self_dependency)
		.expect("contract should deserialize")
		.validate()
		.expect_err("self dependency must be rejected");

	assert!(error.to_string().contains("must not depend on itself"));

	let mut cyclic =
		serde_json::to_value(latent_research_contract_fixture()).expect("fixture should encode");
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

#[test]
fn removed_flat_proposed_issue_summaries_are_rejected() {
	let mut payload =
		serde_json::to_value(latent_research_contract_fixture()).expect("fixture should encode");
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

#[test]
fn latent_contracts_reject_promotion_metadata() {
	let mut contract = latent_research_contract_fixture();

	contract.promotion = Some(sample_promotion());

	assert!(contract.validate().is_err());
}

#[test]
fn validation_rejects_empty_optional_boundary_values() {
	let mut contract = latent_research_contract_fixture();

	contract.links.generated_issue_identifiers.push(String::from(" "));

	assert!(contract.validate().is_err());

	let mut contract = latent_research_contract_fixture();

	contract.evidence_boundary.public_summary = Some(String::new());

	assert!(contract.validate().is_err());

	let mut contract = latent_research_contract_fixture();

	contract.evidence_boundary.private_evidence_refs[0].record_id = Some(0);

	assert!(contract.validate().is_err());
}

#[test]
fn generated_execution_links_are_normalized_and_validated() {
	let mut contract = latent_research_contract_fixture();

	contract
		.link_generated_execution_surfaces(
			[" issue-1 ", "issue-1", "issue-2"],
			["XY-1", " XY-2 "],
			["node-1", "node-1"],
		)
		.expect("links should attach");

	assert_eq!(contract.links().generated_issue_ids(), &["issue-1", "issue-2"]);
	assert_eq!(contract.links().generated_issue_identifiers(), &["XY-1", "XY-2"]);
	assert_eq!(contract.links().execution_program_node_ids(), &["node-1"]);
	assert!(contract.link_generated_execution_surfaces([" "], ["XY-1"], ["node-1"]).is_err());
}

#[test]
fn failed_non_promotion_transitions_leave_contract_unchanged() {
	let mut contract = latent_research_contract_fixture();
	let before_failed_human_decision = contract.clone();

	assert!(contract.require_human_decision(" ").is_err());
	assert_eq!(
		contract, before_failed_human_decision,
		"failed human-decision transition must not mutate the contract"
	);

	let before_failed_rejection = contract.clone();

	assert!(contract.reject_or_supersede(Some(String::from(" "))).is_err());
	assert_eq!(
		contract, before_failed_rejection,
		"failed rejection transition must not mutate the contract"
	);
}
