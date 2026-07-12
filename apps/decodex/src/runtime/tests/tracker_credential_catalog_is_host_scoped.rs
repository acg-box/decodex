use std::fs;

use tempfile::TempDir;

use crate::runtime::{self, tests};

#[test]
fn tracker_credential_catalog_is_host_scoped() {
	let temp_dir = TempDir::new().expect("temp dir");
	let _home_guard = tests::set_test_home(temp_dir.path());
	let config_path = runtime::global_config_path().expect("global config");
	fs::create_dir_all(config_path.parent().expect("config parent")).expect("config dir");
	fs::write(
		config_path,
		r#"
[[tracker.credentials]]
ref = "linear-primary"
provider = "linear"
api_key_env_var = "LINEAR_API_KEY"
"#,
	)
	.expect("config write");

	let catalog = runtime::tracker_credential_catalog().expect("catalog");
	assert_eq!(catalog.len(), 1);
	assert_eq!(catalog[0].credential_ref, "linear-primary");
	assert_eq!(catalog[0].provider, "linear");
	assert_eq!(catalog[0].api_key_env_var, "LINEAR_API_KEY");
}
