use std::fs;

use crate::{
	state::RUN_CONTROL_CHANNEL_DIR,
	worktree::{WorktreeManager, tests},
};

#[test]
fn removes_orphaned_run_control_marker_directory_without_linked_git_metadata() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let orphan_path = worktree_root.join("PUB-102");
	let control_dir = orphan_path.join(RUN_CONTROL_CHANNEL_DIR);
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	fs::create_dir_all(&control_dir).expect("run-control marker directory should exist");
	fs::write(control_dir.join("run-102-1.channel"), "channel\n")
		.expect("run-control marker file should write");

	assert!(
		manager
			.remove_worktree_path(&orphan_path)
			.expect("run-control marker directory should remove")
	);
	assert!(!orphan_path.exists(), "run-control marker directory should be deleted");
}
