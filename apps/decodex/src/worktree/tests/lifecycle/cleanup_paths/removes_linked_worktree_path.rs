use crate::worktree::{WorktreeManager, tests};

#[test]
fn removes_linked_worktree_path() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should exist");

	assert!(manager.remove_worktree_path(&spec.path).expect("worktree should remove"));
	assert!(!spec.path.exists());
	assert!(
		!tests::git_stdout(&repo_root, &["worktree", "list", "--porcelain"])
			.contains(&format!("worktree {}", spec.path.display()))
	);
}
