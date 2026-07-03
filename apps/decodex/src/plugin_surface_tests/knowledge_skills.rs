use crate::plugin_surface_tests::{
	self, KNOWLEDGE_DOCS_DRIFT_REF, KNOWLEDGE_DOCS_DRIFT_SKILL, KNOWLEDGE_DOCS_METHOD_REF,
	KNOWLEDGE_DOCS_OKF_REF, KNOWLEDGE_DOCS_SKILL, KNOWLEDGE_DOCS_WIKI_REF, KNOWLEDGE_OKF_LAYER_REF,
	KNOWLEDGE_OKF_SKILL, KNOWLEDGE_REPO_MEMORY_SKILL, KNOWLEDGE_WRITEBACK_SKILL, ROUTING_REF,
};

#[test]
fn packaged_knowledge_skills_encode_okf_wiki_and_drift_boundaries() {
	let docs_surface = format!(
		"{KNOWLEDGE_DOCS_SKILL}\n{KNOWLEDGE_DOCS_DRIFT_SKILL}\n{KNOWLEDGE_OKF_SKILL}\n{KNOWLEDGE_REPO_MEMORY_SKILL}\n{KNOWLEDGE_WRITEBACK_SKILL}\n{KNOWLEDGE_DOCS_METHOD_REF}\n{KNOWLEDGE_DOCS_OKF_REF}\n{KNOWLEDGE_DOCS_WIKI_REF}\n{KNOWLEDGE_DOCS_DRIFT_REF}\n{KNOWLEDGE_OKF_LAYER_REF}\n{ROUTING_REF}"
	);

	plugin_surface_tests::assert_contains(&docs_surface, "OKF");
	plugin_surface_tests::assert_contains(&docs_surface, "LLM Wiki");
	plugin_surface_tests::assert_contains(&docs_surface, "docs check");
	plugin_surface_tests::assert_contains(&docs_surface, "Markdown-only");
	plugin_surface_tests::assert_contains(&docs_surface, "Research Contract");
	plugin_surface_tests::assert_contains(&docs_surface, "Drift Audit");
	plugin_surface_tests::assert_contains(&docs_surface, "docs impact");
	plugin_surface_tests::assert_contains(&docs_surface, "research_required");
	plugin_surface_tests::assert_contains(&docs_surface, "one authoritative concept per claim");
	plugin_surface_tests::assert_contains_normalized(
		&docs_surface,
		"superseded research routes as provenance",
	);
	plugin_surface_tests::assert_contains(&docs_surface, "`docs/evidence`");
	plugin_surface_tests::assert_contains(&docs_surface, "pass`, `fail`, or `needs-human");
	plugin_surface_tests::assert_contains(
		&docs_surface,
		"Do not create a parallel `wiki/` or `okf/` root",
	);
	plugin_surface_tests::assert_not_contains(&docs_surface, "Okf");
	plugin_surface_tests::assert_contains(&docs_surface, "portable OKF");
	plugin_surface_tests::assert_contains(&docs_surface, "Decodex docs profile");
	plugin_surface_tests::assert_contains(
		&docs_surface,
		"Close the loop between implementation and durable knowledge",
	);
	plugin_surface_tests::assert_contains(&docs_surface, "Prefer automatic writeback");
}

#[test]
fn packaged_okf_skills_preserve_portable_profile_boundary() {
	let okf_surface =
		format!("{KNOWLEDGE_OKF_SKILL}\n{KNOWLEDGE_REPO_MEMORY_SKILL}\n{KNOWLEDGE_OKF_LAYER_REF}");

	plugin_surface_tests::assert_contains(&okf_surface, "portable OKF");
	plugin_surface_tests::assert_contains(&okf_surface, "LLM Wiki");
	plugin_surface_tests::assert_contains(&okf_surface, "repo-memory");
	plugin_surface_tests::assert_contains(&okf_surface, "source-backed repository memory");
	plugin_surface_tests::assert_contains(&okf_surface, "The CLI does not replace the LLM");
	plugin_surface_tests::assert_contains(&okf_surface, "decodex okf init");
	plugin_surface_tests::assert_contains(&okf_surface, "decodex okf check");
	plugin_surface_tests::assert_contains(&okf_surface, "decodex okf find");
	plugin_surface_tests::assert_contains(&okf_surface, "decodex okf graph");
	plugin_surface_tests::assert_contains(&okf_surface, "core");
	plugin_surface_tests::assert_contains(&okf_surface, "wiki");
	plugin_surface_tests::assert_contains(&okf_surface, "repo-memory");
	plugin_surface_tests::assert_contains(&okf_surface, "decodex");
	plugin_surface_tests::assert_contains(&okf_surface, "source_refs");
	plugin_surface_tests::assert_contains(&okf_surface, "code_refs");
	plugin_surface_tests::assert_contains(&okf_surface, "related");
	plugin_surface_tests::assert_contains(&okf_surface, "drift_watch");
	plugin_surface_tests::assert_contains_normalized(
		&okf_surface,
		"Do not create or recommend `decodex docs okf ...`",
	);
	plugin_surface_tests::assert_contains_normalized(&okf_surface, "do not inherit runtime lanes");
	plugin_surface_tests::assert_contains(&okf_surface, "docs-impact checkpoints");
}
