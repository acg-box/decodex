use tempfile::TempDir;

use crate::docs_okf::{
	self, DocsCheckScope,
	tests::{self},
};

#[test]
fn docs_check_rejects_mis_capitalized_okf_acronym() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	tests::write_minimal_okf_bundle(&docs);
	tests::write(
		&docs.join("policy.md"),
		"---\ntype: Policy\ntitle: Okf policy\ndescription: Test concept.\nstatus: active\nauthority: non_authoritative\nowner: docs\nlast_verified: 2026-06-16\n---\n\n# Purpose\nOkf should be uppercase.\n",
	);

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("use `OKF`")));
}
