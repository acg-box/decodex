//! Validate an official export or an installed binary's generated schema without live auth.

use decodex_codex::schema::GeneratedSchemaEvidence;

#[test]
#[ignore = "set DECODEX_REVIEW_SCHEMA to an official experimental JSON schema directory"]
fn official_schema_supports_current_consumers() {
	let directory = std::env::var_os("DECODEX_REVIEW_SCHEMA").expect("schema directory required");
	let evidence = GeneratedSchemaEvidence::load(std::path::Path::new(&directory))
		.expect("official schema must pass bounded loading and account callback validation");
	let contract = evidence.contract();
	assert!(evidence.supports_standalone_tool_output());
	contract.check_conversation_contract().unwrap();
	assert!(contract.advertises_collaboration());
	assert!(contract.advertises_paginated_history());
	for method in ["thread/read", "thread/turns/list", "thread/items/list", "turn/steer"] {
		assert!(contract.advertises_request(method), "missing {method}");
	}
}
