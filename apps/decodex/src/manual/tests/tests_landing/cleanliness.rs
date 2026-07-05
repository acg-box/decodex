use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{
	manual::{self, tests},
	state::{RUN_ACTIVITY_MARKER_FILE, RUN_CONTROL_CHANNEL_DIR},
};

#[test]
fn landing_cleanliness_ignores_untracked_decodex_runtime_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");

	fs::write(checkout.join(RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
		.expect("activity marker should write");

	let control_dir = checkout.join(RUN_CONTROL_CHANNEL_DIR);

	fs::create_dir_all(&control_dir).expect("run-control directory should create");
	fs::write(control_dir.join("run-1-1.channel"), "schema=decodex.run_control_channel/v1\n")
		.expect("run-control channel should write");
	manual::ensure_clean_worktree(&checkout)
		.expect("untracked Decodex runtime artifacts should not block landing");
}

#[test]
fn landing_cleanliness_rejects_blocking_worktree_statuses() {
	fn assert_blocks(checkout: &Path, case_name: &str) {
		let error = manual::ensure_clean_worktree(checkout).expect_err(case_name);

		assert!(
			error.to_string().contains("uncommitted changes"),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");

		fs::write(checkout.join("scratch.txt"), "debug\n").expect("scratch file should write");

		assert_blocks(&checkout, "untracked non-runtime files should block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");
		let nested_dir = checkout.join("nested");

		fs::create_dir_all(&nested_dir).expect("nested directory should create");
		fs::write(nested_dir.join(RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
			.expect("nested activity marker should write");

		assert_blocks(&checkout, "nested runtime marker should still block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");
		let nested_control_dir = checkout.join("nested").join(RUN_CONTROL_CHANNEL_DIR);

		fs::create_dir_all(&nested_control_dir).expect("nested control directory should create");
		fs::write(nested_control_dir.join("run-1-1.channel"), "channel\n")
			.expect("nested control channel should write");

		assert_blocks(&checkout, "nested run-control directory should still block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");
		let marker_path = checkout.join(RUN_ACTIVITY_MARKER_FILE);

		fs::write(&marker_path, "idle\n").expect("activity marker should write");
		tests::git_add_and_commit(
			&checkout,
			RUN_ACTIVITY_MARKER_FILE,
			"track activity marker for test",
		);
		fs::write(&marker_path, "agent_run\n").expect("activity marker should update");

		assert_blocks(&checkout, "tracked runtime marker changes should block landing");
	}
}
