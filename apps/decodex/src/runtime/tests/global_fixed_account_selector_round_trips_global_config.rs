use std::fs;

use tempfile::TempDir;

use crate::runtime::{self, tests};

#[test]
fn global_fixed_account_selector_round_trips_global_config() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard = tests::set_test_home(temp_dir.path());

	assert_eq!(
		runtime::global_fixed_account_selector().expect("missing selector should read"),
		None
	);

	runtime::write_global_fixed_account_selector(Some("copy@example.com"))
		.expect("selector should write");

	assert_eq!(
		runtime::global_fixed_account_selector().expect("selector should read"),
		Some(String::from("copy@example.com"))
	);

	let global_config = fs::read_to_string(
		runtime::global_config_path().expect("global config path should resolve"),
	)
	.expect("global config should exist");

	assert!(global_config.contains("[codex.accounts]"));
	assert!(global_config.contains("fixed_account = \"copy@example.com\""));

	runtime::write_global_fixed_account_selector(None).expect("selector should clear");

	assert_eq!(
		runtime::global_fixed_account_selector().expect("cleared selector should read"),
		None
	);

	let global_config = fs::read_to_string(
		runtime::global_config_path().expect("global config path should resolve"),
	)
	.expect("global config should still exist");

	assert!(!global_config.contains("fixed_account"));
}
