use std::fs;

use tempfile::TempDir;

use crate::{
	runtime,
	runtime::tests::{self},
	state::StateStore,
};

#[test]
fn project_config_registration_requires_explicit_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = tests::set_test_home(temp_dir.path());
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let config_dir =
		runtime::project_config_dir().expect("project config dir should resolve").join("pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&config_dir).expect("project config dir should exist");
	tests::write_workflow(&config_dir);
	tests::write_config_without_repo_root(&config_path);

	let error = runtime::register_project_config(&state_store, &config_dir, true)
		.expect_err("centralized project config without repo_root should fail");

	assert!(
		error.to_string().contains("paths.repo_root"),
		"error should explain the missing explicit repo root: {error:?}"
	);
}
