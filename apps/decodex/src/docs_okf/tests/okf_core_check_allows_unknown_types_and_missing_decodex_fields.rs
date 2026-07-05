use std::fs;

use tempfile::TempDir;

use crate::docs_okf::{self, OkfCheckProfile, tests};

#[test]
fn okf_core_check_allows_unknown_types_and_missing_decodex_fields() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("bundle");

	fs::create_dir_all(&bundle).expect("bundle");
	tests::write(&bundle.join("index.md"), "# Bundle\n");
	tests::write(
		&bundle.join("metric.md"),
		"---\ntype: Business Metric\n---\n\nWeekly active users.\n",
	);

	let report = docs_okf::run_okf_check(&bundle, OkfCheckProfile::Core).expect("core check");

	assert!(!report.has_issues(), "{report:#?}");
}
