use std::fs;

use tempfile::TempDir;

use crate::{
	manual::{self},
	state::StateStore,
};

#[test]
fn manual_commit_blocker_rejects_active_claimed_managed_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = temp_dir.path().join("XY-225");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"decodex",
			"issue-1",
			"y/decodex-xy-225",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should persist");
	state_store
		.upsert_lease("decodex", "issue-1", "run-1", "In Progress")
		.expect("active lease should persist");

	let blocker = manual::manual_commit_active_lane_blocker(
		&state_store,
		"decodex",
		&worktree_path,
		Some("y/decodex-xy-225"),
	)
	.expect("manual commit blocker should evaluate")
	.expect("active managed worktree should block");

	assert_eq!(blocker.issue_id, "issue-1");
	assert_eq!(blocker.branch_name, "y/decodex-xy-225");
	assert_eq!(blocker.worktree_path, worktree_path);
}

#[test]
fn manual_commit_blocker_allows_unclaimed_or_unmapped_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = temp_dir.path().join("XY-225");
	let other_path = temp_dir.path().join("XY-226");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::create_dir_all(&other_path).expect("other worktree path should exist");

	state_store
		.upsert_worktree(
			"decodex",
			"issue-1",
			"y/decodex-xy-225",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should persist");

	assert!(
		manual::manual_commit_active_lane_blocker(
			&state_store,
			"decodex",
			&worktree_path,
			Some("y/decodex-xy-225"),
		)
		.expect("unclaimed worktree should evaluate")
		.is_none()
	);

	state_store
		.upsert_lease("decodex", "issue-1", "run-1", "In Progress")
		.expect("active lease should persist");

	assert!(
		manual::manual_commit_active_lane_blocker(
			&state_store,
			"decodex",
			&other_path,
			Some("y/decodex-xy-226"),
		)
		.expect("unmapped worktree should evaluate")
		.is_none()
	);
}
