use crate::loop_contract::tests;

#[test]
fn validation_rejects_empty_optional_boundary_values() {
	let mut contract = tests::latent_research_contract_fixture();

	contract.links.generated_issue_identifiers.push(String::from(" "));

	assert!(contract.validate().is_err());

	let mut contract = tests::latent_research_contract_fixture();

	contract.evidence_boundary.public_summary = Some(String::new());

	assert!(contract.validate().is_err());

	let mut contract = tests::latent_research_contract_fixture();

	contract.evidence_boundary.private_evidence_refs[0].record_id = Some(0);

	assert!(contract.validate().is_err());
}
