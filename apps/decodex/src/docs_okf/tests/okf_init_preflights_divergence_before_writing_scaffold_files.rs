use std::fs;

use tempfile::TempDir;

use crate::docs_okf::{self, OkfCheckProfile, tests};

#[test]
fn okf_init_preflights_divergence_before_writing_scaffold_files() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("knowledge");

	fs::create_dir_all(&bundle).expect("bundle");
	tests::write(&bundle.join("overview.md"), "# Existing Overview\n");

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
