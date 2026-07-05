use std::{fs, path::PathBuf};

use crate::worktree::{WorktreeManager, git, hooks, tests};

#[test]
fn creates_linked_worktree() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(spec.branch_name, "x/pubfi-pub-101");
	assert!(spec.path.join(".git").is_file());
	assert_eq!(
		tests::git_stdout(&spec.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
		"x/pubfi-pub-101"
	);

	let repo_git_dir = fs::canonicalize(repo_root.join(".git")).expect("repo git dir");
	let git_dir = fs::canonicalize(PathBuf::from(tests::git_stdout(
		&spec.path,
		&["rev-parse", "--path-format=absolute", "--git-dir"],
	)))
	.expect("git dir should canonicalize");
	let git_common_dir = fs::canonicalize(PathBuf::from(tests::git_stdout(
		&spec.path,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
	)))
	.expect("git common dir should canonicalize");

	assert!(git_dir.starts_with(repo_git_dir.join("worktrees")));
	assert_eq!(git_common_dir, repo_git_dir);
	assert!(
		git::worktree_is_registered(
			&repo_root,
			&fs::canonicalize(&spec.path).expect("canonical worktree path")
		)
		.expect("worktree registration should inspect")
	);
}

#[test]
fn after_create_hook_runs_only_for_new_worktree() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let hook_log = repo_root.join("after-create.log");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = ["printf '%s\n' \"$DECODEX_BRANCH\" >> \"$DECODEX_REPO_ROOT/after-create.log\""]
before_remove_commands = []
timeout_seconds = 60
			"#,
	);
	let created = manager
		.ensure_worktree_with_hooks("PUB-101", false, &hooks)
		.expect("worktree should be created");
	let reused = manager
		.ensure_worktree_with_hooks("PUB-101", false, &hooks)
		.expect("worktree should be reused");

	assert!(!created.reused_existing);
	assert!(reused.reused_existing);
	assert_eq!(fs::read_to_string(&hook_log).expect("hook log should exist"), "x/pubfi-pub-101\n");
}

#[test]
fn after_create_hook_failure_keeps_created_worktree() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = ["exit 23"]
before_remove_commands = []
timeout_seconds = 60
			"#,
	);
	let planned = manager.plan_for_issue("PUB-101");
	let error = manager
		.ensure_worktree_with_hooks("PUB-101", false, &hooks)
		.expect_err("after_create hook failure should stop setup");

	assert!(error.to_string().contains("Workspace hook `after_create` command `exit 23` failed"));
	assert!(planned.path.exists(), "failed hook should keep the worktree for inspection");
	assert!(planned.path.join(".git").is_file(), "failed hook should keep the linked worktree");
	assert!(
		hooks::after_create_pending_marker_path(&planned.path).exists(),
		"failed after-create hook should leave a pending bootstrap marker"
	);
}
