use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn before_remove_hook_does_not_run_for_unregistered_directory() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let rogue_path = worktree_root.join("PUB-rogue");
	let hook_log = repo_root.join("before-remove.log");

	fs::create_dir_all(&rogue_path).expect("rogue path should exist");
	fs::write(rogue_path.join(".git"), b"not-a-worktree\n")
		.expect("rogue path should contain a fake git pointer");

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf 'hook-ran\n' > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60
			"#,
	);
	let error = manager
		.remove_worktree_path_with_hooks("PUB-rogue", "x/pubfi-pub-rogue", &rogue_path, &hooks)
		.expect_err("unregistered directory should fail validation before before_remove hooks");

	assert!(
		!error.to_string().trim().is_empty(),
		"validation failure should still surface an actionable error"
	);
	assert!(
		!hook_log.exists(),
		"before_remove hook should not run before linked worktree validation succeeds"
	);
	assert!(
		rogue_path.exists(),
		"failed validation should leave the unregistered directory untouched"
	);
}
