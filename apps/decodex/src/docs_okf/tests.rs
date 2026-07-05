mod docs_check_passes_minimal_okf_bundle;
mod docs_check_rejects_invalid_frontmatter_values;
mod docs_check_rejects_invalid_structured_frontmatter_refs;
mod docs_check_rejects_json_artifacts;
mod docs_check_rejects_json_artifacts_inside_research;
mod docs_check_rejects_mis_capitalized_okf_acronym;
mod docs_check_rejects_non_markdown_artifacts;
mod okf_core_check_allows_unknown_types_and_missing_decodex_fields;
mod okf_graph_skips_links_outside_the_bundle;
mod okf_init_is_idempotent_for_unchanged_scaffold_files;
mod okf_init_preflights_divergence_before_writing_scaffold_files;
mod okf_init_refuses_to_overwrite_existing_content;
mod okf_init_rejects_decodex_profile;
mod okf_init_scaffolds_repo_memory_bundle_that_passes_check;
mod okf_query_matches_structured_frontmatter_refs;

use std::{fs, path::Path};

fn write_minimal_okf_bundle(docs: &Path) {
	for lane in ["decisions", "evidence", "reference", "research", "runbook", "spec"] {
		fs::create_dir_all(docs.join(lane)).expect("dirs");
	}

	write(&docs.join("index.md"), "# Docs\n\n* [Policy](policy.md)\n");
	write(&docs.join("log.md"), "# Log\n");
	write(&docs.join("policy.md"), concept("Policy", "Docs policy"));
	write(&docs.join("decisions/index.md"), "# Decisions\n");
	write(&docs.join("evidence/index.md"), "# Evidence\n\n* [Docs drift](docs-drift.md)\n");
	write(&docs.join("evidence/docs-drift.md"), drift_concept("Docs drift"));
	write(&docs.join("reference/index.md"), "# Reference\n");
	write(&docs.join("research/index.md"), "# Research\n");
	write(&docs.join("runbook/index.md"), "# Runbooks\n");
	write(&docs.join("spec/index.md"), "# Specs\n");
}

fn concept(concept_type: &str, title: &str) -> String {
	format!(
		"---\ntype: {concept_type}\ntitle: {title}\ndescription: Test concept.\nstatus: active\nauthority: non_authoritative\nowner: docs\nlast_verified: 2026-06-16\n---\n\n# Purpose\nTest.\n"
	)
}

fn drift_concept(title: &str) -> String {
	format!(
		"---\ntype: Drift Audit\ntitle: {title}\ndescription: Test drift audit.\nstatus: active\nauthority: evidence\nowner: docs\nlast_verified: 2026-06-16\n---\n\n# {title}\n\n## Watched Claims\nTest.\n\n## Evidence Anchors\nTest.\n\n## Reverse Checks\nTest.\n\n## Verdict\npass\n\n## Required Updates\nNone.\n\n## Citations\nNone.\n"
	)
}

fn write(path: &Path, content: impl AsRef<str>) {
	fs::write(path, content.as_ref()).expect("write");
}
