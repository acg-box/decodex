use tempfile::TempDir;

use crate::docs_okf::{
	self, DocsCheckScope,
	tests::{self},
};

#[test]
fn docs_check_rejects_non_markdown_artifacts() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	tests::write_minimal_okf_bundle(&docs);
	tests::write(&docs.join("stray.txt"), "not OKF\n");

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| issue.message.contains("only .md files")));
}
