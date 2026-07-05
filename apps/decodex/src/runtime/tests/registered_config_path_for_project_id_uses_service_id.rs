use std::fs;

use tempfile::TempDir;

use crate::{
	runtime,
	runtime::tests::{self},
	state::StateStore,
};

#[test]
fn registered_config_path_for_project_id_uses_service_id() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = temp_dir.path().join("target-repo");
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let config_dir = temp_dir.path().join("projects/pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&config_dir).expect("project config dir should exist");
	tests::write_workflow(&config_dir);
	tests::write_config_body(&config_path, &repo_root);
	runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should register");

	assert_eq!(
		runtime::registered_config_path_for_project_id(&state_store, "pubfi")
			.expect("project id lookup should succeed"),
		fs::canonicalize(&config_path).expect("config should canonicalize")
	);
	assert!(
		runtime::registered_config_path_for_project_id(&state_store, "missing")
			.expect_err("unknown project id should fail")
			.to_string()
			.contains("Registered projects: pubfi")
	);
}
