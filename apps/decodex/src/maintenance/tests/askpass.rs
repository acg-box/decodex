use std::{
	fs::{self},
	time::{Duration, SystemTime},
};

use tempfile::TempDir;

use crate::{
	maintenance::{
		self, MaintenanceMode, MaintenancePolicy, MaintenancePruneRequest, MaintenanceScope, tests,
	},
	test_support::TestEnvVarGuard,
};

#[test]
fn prune_deletes_old_legacy_git_askpass_helpers_from_registered_worktree_roots() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard =
		TestEnvVarGuard::set("HOME", temp_dir.path().to_str().expect("home should be utf-8"));
	let connection = tests::bootstrap_test_runtime_db(&temp_dir);
	let worktree_root = temp_dir.path().join("repo/.worktrees");
	let old_helper = worktree_root.join(".decodex-git-askpass-xy-101-attempt-1.sh");
	let fresh_helper = worktree_root.join(".decodex-git-askpass-xy-102-attempt-1.sh");
	let unrelated = worktree_root.join("notes.sh");
	let old_time = SystemTime::now() - Duration::from_secs(2 * 24 * 60 * 60);
	let fresh_time = SystemTime::now();

	tests::insert_project(&connection, &worktree_root);
	fs::create_dir_all(&worktree_root).expect("worktree root should create");
	fs::write(&old_helper, b"#!/bin/sh\n").expect("old helper should write");
	fs::write(&fresh_helper, b"#!/bin/sh\n").expect("fresh helper should write");
	fs::write(&unrelated, b"#!/bin/sh\n").expect("unrelated file should write");
	tests::set_file_modified(&old_helper, old_time);
	tests::set_file_modified(&fresh_helper, fresh_time);
	tests::set_file_modified(&unrelated, old_time);

	let report = maintenance::run_prune_with_policy(
		MaintenancePruneRequest {
			mode: MaintenanceMode::Apply,
			scope: MaintenanceScope::AutoSafe,
			json: false,
		},
		MaintenancePolicy::default(),
	)
	.expect("maintenance should run");

	assert_eq!(report.git_askpass_helpers.deleted_files, 1);
	assert_eq!(report.git_askpass_helpers.delete_candidates, 1);
	assert!(!old_helper.exists());
	assert!(fresh_helper.exists());
	assert!(unrelated.exists());
}
