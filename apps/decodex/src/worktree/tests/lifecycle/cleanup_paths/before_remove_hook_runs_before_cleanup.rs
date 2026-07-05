use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn before_remove_hook_runs_before_cleanup() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let hook_log = repo_root.join("before-remove.log");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf '%s:%s\n' \"$DECODEX_ISSUE_ID\" \"$DECODEX_BRANCH\" > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60
			"#,
	);
	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should exist");

	assert!(
		manager
			.remove_worktree_path_with_hooks(
				&spec.issue_identifier,
				&spec.branch_name,
				&spec.path,
				&hooks
			)
			.expect("worktree should remove")
	);
	assert_eq!(
		fs::read_to_string(&hook_log).expect("hook log should exist"),
		"PUB-101:x/pubfi-pub-101\n"
	);
	assert!(!spec.path.exists(), "successful cleanup should still remove the worktree");
}
