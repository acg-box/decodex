use tempfile::TempDir;

use crate::docs_okf::{
	self, DocsCheckScope,
	tests::{self},
};

#[test]
fn docs_check_rejects_invalid_structured_frontmatter_refs() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	tests::write_minimal_okf_bundle(&docs);
	tests::write(
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
