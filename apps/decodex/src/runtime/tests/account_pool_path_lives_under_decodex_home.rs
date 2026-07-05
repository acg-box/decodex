use tempfile::TempDir;

use crate::runtime::{self, tests};

#[test]
fn account_pool_path_lives_under_decodex_home() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let _home_guard = tests::set_test_home(temp_dir.path());

	assert_eq!(
		runtime::accounts_path().expect("accounts path should resolve"),
		temp_dir.path().join(".codex/decodex/accounts.jsonl")
	);
}
