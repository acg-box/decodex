use std::fs;

use crate::worktree::{WorktreeManager, tests};

#[test]
fn linked_worktree_uses_frozen_admitted_base_after_source_advances() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let admitted_base = manager.source_head_oid().expect("admitted base");

	fs::write(repo_root.join("README.md"), "advanced\n").expect("advance source");
	tests::run_git(&repo_root, &["add", "README.md"]);
	tests::run_git(&repo_root, &["commit", "-m", "advance source"]);
	let advanced_head = manager.source_head_oid().expect("advanced head");
	assert_ne!(advanced_head, admitted_base);

	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60
		"#,
	);
	let worktree = manager
		.ensure_worktree_with_hooks_at_base("PUB-101", false, &hooks, &admitted_base)
		.expect("create from admitted base");

	assert_eq!(tests::git_stdout(&worktree.path, &["rev-parse", "HEAD"]), admitted_base);
}
