use std::fs;

use crate::{
	state::RUN_ACTIVITY_MARKER_FILE,
	worktree::{WorktreeManager, tests},
};

#[test]
fn removes_orphaned_marker_directory_without_linked_git_metadata() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let orphan_path = worktree_root.join("PUB-101");
	let hook_log = repo_root.join("before-remove.log");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf 'hook-ran\n' > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60
			"#,
	);

	fs::create_dir_all(&orphan_path).expect("orphan path should exist");
	fs::write(orphan_path.join(RUN_ACTIVITY_MARKER_FILE), "run_id=run-orphan\n")
		.expect("runtime marker should write");

	assert!(
		manager
			.remove_worktree_path_with_hooks("PUB-101", "x/pubfi-pub-101", &orphan_path, &hooks,)
			.expect("orphan marker directory should remove")
	);
	assert!(!orphan_path.exists(), "orphan marker directory should be deleted");
	assert!(
		!hook_log.exists(),
		"before_remove hook should not run for a non-worktree marker directory"
	);
}
