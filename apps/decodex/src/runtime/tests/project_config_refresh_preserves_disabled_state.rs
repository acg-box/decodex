use std::fs;

use tempfile::TempDir;

use crate::{
	runtime,
	runtime::tests::{self},
	state::StateStore,
};

#[test]
fn project_config_refresh_preserves_disabled_state() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = tests::set_test_home(temp_dir.path());
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let repo_root = temp_dir.path().join("target-repo");
	let config_dir =
		runtime::project_config_dir().expect("project config dir should resolve").join("pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&config_dir).expect("project config dir should exist");
	tests::write_workflow(&config_dir);
	tests::write_config_body(&config_path, &repo_root);
	runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should register");

	state_store.set_project_enabled("pubfi", false).expect("project should disable");

	let registration = runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should refresh");
	let projects = state_store.list_projects().expect("projects should list");

	assert!(!registration.enabled(), "runtime refresh should report the preserved disabled state");
	assert_eq!(projects.len(), 1, "refresh should keep one project row");
	assert!(!projects[0].enabled(), "stored project should remain disabled");
}
