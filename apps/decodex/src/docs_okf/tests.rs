use std::fs;

use tempfile::TempDir;

use crate::docs_okf::{self, DocsCheckScope, OkfCheckProfile, OkfQuery};

#[test]
fn docs_check_rejects_json_artifacts() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	write_minimal_okf_bundle(&docs);
	write(&docs.join("research.json"), "{}\n");

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("JSON artifacts")));
}

#[test]
fn docs_check_rejects_json_artifacts_inside_research() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	write_minimal_okf_bundle(&docs);
	write(&docs.join("research/sample-report.json"), "{}\n");

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| {
		issue.path.as_deref() == Some(std::path::Path::new("research/sample-report.json"))
			&& issue.message.contains("JSON artifacts")
	}));
}

#[test]
fn docs_check_rejects_mis_capitalized_okf_acronym() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	write_minimal_okf_bundle(&docs);
	write(
		&docs.join("policy.md"),
		"---\ntype: Policy\ntitle: Okf policy\ndescription: Test concept.\nstatus: active\nauthority: non_authoritative\nowner: docs\nlast_verified: 2026-06-16\n---\n\n# Purpose\nOkf should be uppercase.\n",
	);

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("use `OKF`")));
}

#[test]
fn docs_check_passes_minimal_okf_bundle() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	write_minimal_okf_bundle(&docs);

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(!report.has_issues(), "{report:#?}");
}

#[test]
fn docs_check_rejects_non_markdown_artifacts() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	write_minimal_okf_bundle(&docs);
	write(&docs.join("stray.txt"), "not OKF\n");

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("only .md files")));
}

#[test]
fn docs_check_rejects_invalid_frontmatter_values() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	write_minimal_okf_bundle(&docs);
	write(
		&docs.join("policy.md"),
		"---\ntype: Policy\ntitle: Docs policy\ndescription: Test concept.\nstatus: nonsense\nauthority: non_authoritative\nowner: docs\nlast_verified: yesterday\n---\n\n# Purpose\nTest.\n",
	);

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("unsupported value")));
	assert!(report.issues.iter().any(|issue| issue.message.contains("must be an ISO date")));
}

#[test]
fn docs_check_rejects_invalid_structured_frontmatter_refs() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	write_minimal_okf_bundle(&docs);
	write(
		&docs.join("policy.md"),
		"---\ntype: Policy\ntitle: Docs policy\ndescription: Test concept.\nstatus: active\nauthority: non_authoritative\nowner: docs\nlast_verified: 2026-06-16\nsource_refs: ['https://', not-a-url]\ncode_refs: [missing.rs, docs/]\nrelated: [missing.md, '#heading']\npromotes_to: [docs/research]\ndrift_watch: not-a-list\n---\n\n# Purpose\nTest.\n",
	);

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("http(s) URL")));
	assert!(report.issues.iter().any(|issue| issue.message.contains("code_refs entry")));
	assert!(report.issues.iter().any(|issue| issue.message.contains("related entry")));
	assert!(report.issues.iter().any(|issue| issue.message.contains("promotes_to entry")));
	assert!(report.issues.iter().any(|issue| issue.message.contains("drift_watch")));
}

#[test]
fn okf_core_check_allows_unknown_types_and_missing_decodex_fields() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("bundle");

	fs::create_dir_all(&bundle).expect("bundle");

	write(&bundle.join("index.md"), "# Bundle\n");
	write(&bundle.join("metric.md"), "---\ntype: Business Metric\n---\n\nWeekly active users.\n");

	let report = docs_okf::run_okf_check(&bundle, OkfCheckProfile::Core).expect("core check");

	assert!(!report.has_issues(), "{report:#?}");
}

#[test]
fn okf_graph_skips_links_outside_the_bundle() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("bundle");

	fs::create_dir_all(&bundle).expect("bundle");

	write(&temp_dir.path().join("README.md"), "# External repo doc\n");
	write(&bundle.join("index.md"), "# Bundle\n");
	write(
		&bundle.join("alpha.md"),
		"---\ntype: Concept\ntitle: Alpha\ndescription: Alpha concept.\n---\n\nSee [Beta](beta.md) and [repo readme](../README.md).\n",
	);
	write(
		&bundle.join("beta.md"),
		"---\ntype: Concept\ntitle: Beta\ndescription: Beta concept.\n---\n\nBeta.\n",
	);

	let graph = docs_okf::build_okf_graph(&bundle).expect("graph");

	assert_eq!(graph.broken_links, Vec::new());
	assert_eq!(graph.edges.len(), 1);
	assert_eq!(graph.edges[0].target, "beta");
}

#[test]
fn okf_query_matches_structured_frontmatter_refs() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("docs");

	fs::create_dir_all(&bundle).expect("bundle");

	write(&temp_dir.path().join("src.rs"), "fn main() {}\n");
	write(&bundle.join("index.md"), "# Bundle\n");
	write(
		&bundle.join("alpha.md"),
		"---\ntype: Concept\ntitle: Alpha\ndescription: Alpha concept.\ntags: [runtime]\nsource_refs: [https://example.com/spec]\ncode_refs: [src.rs]\nrelated: [beta.md]\n---\n\nAlpha.\n",
	);
	write(
		&bundle.join("beta.md"),
		"---\ntype: Concept\ntitle: Beta\ndescription: Beta concept.\n---\n\nBeta.\n",
	);

	let query = OkfQuery {
		code_ref: Some(String::from("src.rs")),
		tags: Vec::new(),
		..OkfQuery::default()
	};
	let matches = docs_okf::query_okf_bundle(&bundle, &query).expect("query");

	assert_eq!(matches.len(), 1);
	assert_eq!(matches[0].id, "alpha");
}

#[test]
fn okf_init_scaffolds_repo_memory_bundle_that_passes_check() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("knowledge");
	let init_report =
		docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::RepoMemory).expect("init");
	let check_report =
		docs_okf::run_okf_check(&bundle, OkfCheckProfile::RepoMemory).expect("check");
	let graph = docs_okf::build_okf_graph(&bundle).expect("graph initialized bundle");

	assert_eq!(init_report.profile(), OkfCheckProfile::RepoMemory);
	assert_eq!(init_report.created.len(), 3);
	assert!(init_report.unchanged.is_empty());
	assert!(!check_report.has_issues(), "{check_report:#?}");
	assert!(graph.concepts.iter().any(|concept| concept.id == "overview"));
}

#[test]
fn okf_init_is_idempotent_for_unchanged_scaffold_files() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("knowledge");

	docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Wiki).expect("first init");

	let init_report =
		docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Wiki).expect("second init");

	assert!(init_report.created.is_empty());
	assert_eq!(init_report.unchanged.len(), 3);
}

#[test]
fn okf_init_refuses_to_overwrite_existing_content() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("knowledge");

	fs::create_dir_all(&bundle).expect("bundle");

	write(&bundle.join("index.md"), "# Existing Index\n");

	let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Core)
		.expect_err("init should refuse divergent scaffold files");

	assert!(error.to_string().contains("already exists with different content"));
}

#[test]
fn okf_init_preflights_divergence_before_writing_scaffold_files() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("knowledge");

	fs::create_dir_all(&bundle).expect("bundle");

	write(&bundle.join("overview.md"), "# Existing Overview\n");

	let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::RepoMemory)
		.expect_err("init should refuse before writing partial scaffold files");

	assert!(error.to_string().contains("already exists with different content"));
	assert!(!bundle.join("index.md").exists());
	assert!(!bundle.join("log.md").exists());
	assert_eq!(
		fs::read_to_string(bundle.join("overview.md")).expect("overview"),
		"# Existing Overview\n"
	);
}

#[test]
fn okf_init_rejects_decodex_profile() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("docs");
	let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Decodex)
		.expect_err("portable init should not scaffold decodex profile");

	assert!(error.to_string().contains("portable profiles only"));
}

fn write_minimal_okf_bundle(docs: &std::path::Path) {
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

fn write(path: &std::path::Path, content: impl AsRef<str>) {
	fs::write(path, content.as_ref()).expect("write");
}
