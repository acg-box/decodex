use std::fs;

use tempfile::TempDir;

use crate::{
	runtime,
	runtime::tests::{self},
	state::StateStore,
};

#[test]
fn registered_config_path_for_cwd_matches_repo_and_worktree_roots() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = temp_dir.path().join("target-repo");
	let worktree_root = repo_root.join(".worktrees");
	let lane_root = worktree_root.join("XY-380");
	let state_store =
		StateStore::open(temp_dir.path().join("runtime.sqlite3")).expect("state store should open");
	let config_dir = temp_dir.path().join("projects/pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&lane_root).expect("lane root should exist");
	fs::create_dir_all(&config_dir).expect("project config dir should exist");
	tests::write_workflow(&config_dir);
	tests::write_config_body(&config_path, &repo_root);

	let registration = runtime::register_project_config(&state_store, &config_dir, true)
		.expect("project config should register");
	let canonical_config = fs::canonicalize(&config_path).expect("config should canonicalize");

	assert_eq!(registration.config_path(), canonical_config.as_path());
	assert_eq!(
		runtime::registered_config_path_for_cwd(&state_store, &repo_root)
			.expect("repo cwd lookup should succeed"),
		Some(canonical_config.clone())
	);
	assert_eq!(
		runtime::registered_config_path_for_cwd(&state_store, &lane_root)
			.expect("worktree cwd lookup should succeed"),
		Some(canonical_config)
	);
}
