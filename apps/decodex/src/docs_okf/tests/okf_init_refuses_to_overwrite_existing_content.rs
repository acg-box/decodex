use std::fs;

use tempfile::TempDir;

use crate::docs_okf::{self, OkfCheckProfile, tests};

#[test]
fn okf_init_refuses_to_overwrite_existing_content() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("knowledge");

	fs::create_dir_all(&bundle).expect("bundle");
	tests::write(&bundle.join("index.md"), "# Existing Index\n");

	let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Core)
		.expect_err("init should refuse divergent scaffold files");

	assert!(error.to_string().contains("already exists with different content"));
}
