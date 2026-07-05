use tempfile::TempDir;

use crate::docs_okf::{
	self, DocsCheckScope,
	tests::{self},
};

#[test]
fn docs_check_rejects_json_artifacts_inside_research() {
	let temp_dir = TempDir::new().expect("tempdir");
	let docs = temp_dir.path().join("docs");

	tests::write_minimal_okf_bundle(&docs);
	tests::write(&docs.join("research/sample-report.json"), "{}\n");

	let report = docs_okf::run_docs_check(&docs, DocsCheckScope::All).expect("check");

	assert!(report.has_issues());
	assert!(report.issues.iter().any(|issue| {
		issue.path.as_deref() == Some(std::path::Path::new("research/sample-report.json"))
			&& issue.message.contains("JSON artifacts")
	}));
}
