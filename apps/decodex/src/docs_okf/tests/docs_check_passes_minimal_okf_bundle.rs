use tempfile::TempDir;

use crate::docs_okf::{self, DocsCheckScope, tests};

#[test]
fn docs_check_passes_minimal_okf_bundle() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	tests::write_minimal_okf_bundle(&docs);

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(!report.has_issues(), "{report:#?}");
}
