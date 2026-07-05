use tempfile::TempDir;

use crate::docs_okf::{self, OkfCheckProfile};

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
