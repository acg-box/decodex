use crate::loop_contract::tests;

#[test]
fn generated_execution_links_are_normalized_and_validated() {
	let mut contract = tests::latent_research_contract_fixture();

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
