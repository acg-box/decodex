use tempfile::TempDir;

use crate::docs_okf::{self, OkfCheckProfile};

#[test]
fn okf_init_rejects_decodex_profile() {
	let temp_dir = TempDir::new().expect("tempdir");
	let bundle = temp_dir.path().join("docs");
	let error = docs_okf::init_okf_bundle(&bundle, OkfCheckProfile::Decodex)
		.expect_err("portable init should not scaffold decodex profile");

	assert!(error.to_string().contains("portable profiles only"));
}
