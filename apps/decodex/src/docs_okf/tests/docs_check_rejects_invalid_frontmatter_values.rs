use tempfile::TempDir;

use crate::docs_okf::{
	self, DocsCheckScope,
	tests::{self},
};

#[test]
fn docs_check_rejects_invalid_frontmatter_values() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	tests::write_minimal_okf_bundle(&docs);
	tests::write(
		&docs.join("policy.md"),
		"---\ntype: Policy\ntitle: Docs policy\ndescription: Test concept.\nstatus: nonsense\nauthority: non_authoritative\nowner: docs\nlast_verified: yesterday\n---\n\n# Purpose\nTest.\n",
	);

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("unsupported value")));
	assert!(report.issues.iter().any(|issue| issue.message.contains("must be an ISO date")));
}
