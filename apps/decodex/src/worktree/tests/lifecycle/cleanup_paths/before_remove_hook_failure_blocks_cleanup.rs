use crate::worktree::{WorktreeManager, tests};

#[test]
fn before_remove_hook_failure_blocks_cleanup() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["exit 19"]
timeout_seconds = 60
			"#,
	);
	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should exist");
	let error = manager
		.remove_worktree_path_with_hooks(
			&spec.issue_identifier,
			&spec.branch_name,
			&spec.path,
			&hooks,
		)
		.expect_err("before_remove hook failure should block cleanup");

	assert!(error.to_string().contains("Workspace hook `before_remove` command `exit 19` failed"));
	assert!(spec.path.exists(), "blocked cleanup should keep the worktree");
}
