use crate::plugin_surface_tests::{
	self, CODEBASE_PLUGIN_JSON, CODEBASE_REF, DECODEX_PLUGIN_JSON, DELIBERATION_PLUGIN_JSON,
	KNOWLEDGE_OKF_LAYER_REF, KNOWLEDGE_PLUGIN_JSON, ROUTING_REF,
};

#[test]
fn packaged_plugin_manifests_route_split_surface_owners() {
	let decodex_surface = plugin_surface_tests::manifest_interface_surface(DECODEX_PLUGIN_JSON);
	let knowledge_surface = plugin_surface_tests::manifest_interface_surface(KNOWLEDGE_PLUGIN_JSON);
	let codebase_surface = plugin_surface_tests::manifest_interface_surface(CODEBASE_PLUGIN_JSON);
	let deliberation_surface =
		plugin_surface_tests::manifest_interface_surface(DELIBERATION_PLUGIN_JSON);
	let routing_surface = format!("{ROUTING_REF}\n{KNOWLEDGE_OKF_LAYER_REF}\n{CODEBASE_REF}");

	plugin_surface_tests::assert_contains(&decodex_surface, "bounded research");
	plugin_surface_tests::assert_contains(&decodex_surface, "runtime ops");
	plugin_surface_tests::assert_contains(&decodex_surface, "companion plugins");
	plugin_surface_tests::assert_contains(&decodex_surface, "Research this with Decodex.");
	plugin_surface_tests::assert_contains(&decodex_surface, "Plan accepted Decodex work.");
	plugin_surface_tests::assert_contains(&decodex_surface, "Operate Decodex.");
	plugin_surface_tests::assert_not_contains(&decodex_surface, "Maintain Decodex docs.");
	plugin_surface_tests::assert_not_contains(&decodex_surface, "Work with an OKF bundle.");
	plugin_surface_tests::assert_contains(&knowledge_surface, "OKF and LLM Wiki");
	plugin_surface_tests::assert_contains(&knowledge_surface, "semantic drift audits");
	plugin_surface_tests::assert_contains(&knowledge_surface, "source-backed repository memory");
	plugin_surface_tests::assert_contains(&knowledge_surface, "automatic knowledge writeback");
	plugin_surface_tests::assert_contains(&knowledge_surface, "Maintain docs and OKF.");
	plugin_surface_tests::assert_contains(&knowledge_surface, "Audit semantic drift.");
	plugin_surface_tests::assert_contains(&knowledge_surface, "Write back stable repo knowledge.");
	plugin_surface_tests::assert_contains(&codebase_surface, "task-runner structure");
	plugin_surface_tests::assert_contains(&codebase_surface, "module-boundary defaults");
	plugin_surface_tests::assert_contains(&codebase_surface, "verification evidence");
	plugin_surface_tests::assert_contains(&codebase_surface, "root-cause debugging");
	plugin_surface_tests::assert_contains(&codebase_surface, "Apply codebase rules.");
	plugin_surface_tests::assert_contains(&codebase_surface, "Verify this change.");
	plugin_surface_tests::assert_contains(&codebase_surface, "Debug this failure.");
	plugin_surface_tests::assert_contains(&deliberation_surface, "read-only evidence scouting");
	plugin_surface_tests::assert_contains(&deliberation_surface, "decision grilling");
	plugin_surface_tests::assert_contains(&deliberation_surface, "evidence sufficiency");
	plugin_surface_tests::assert_contains(&deliberation_surface, "Scout this evidence.");
	plugin_surface_tests::assert_contains(&deliberation_surface, "Grill this plan.");
	plugin_surface_tests::assert_contains(&deliberation_surface, "Skeptic-review this claim.");
	plugin_surface_tests::assert_contains(&routing_surface, "$knowledge:docs");
	plugin_surface_tests::assert_contains(&routing_surface, "$knowledge:okf");
	plugin_surface_tests::assert_contains(&routing_surface, "$knowledge:repo-memory");
	plugin_surface_tests::assert_contains(&routing_surface, "$codebase:work");
	plugin_surface_tests::assert_contains(&routing_surface, "$deliberation:skeptic");
	plugin_surface_tests::assert_contains(&routing_surface, "$knowledge:docs-drift");
	plugin_surface_tests::assert_contains(&routing_surface, "$knowledge:writeback");
	plugin_surface_tests::assert_contains(
		&routing_surface,
		"Runtime: `docs/spec/` and `docs/runbook/`",
	);
}
