use std::fs;

use crate::{
	state::{RUN_ACTIVITY_MARKER_FILE, RUN_CONTROL_CHANNEL_DIR},
	worktree::{WorktreeManager, tests},
};
#[test]
fn rejects_reused_non_worktree_checkout_with_embedded_git_dir() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("PUB-101");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");
	tests::run_git(
		&repo_root,
		&["clone", "--quiet", "--no-checkout", ".", worktree_path.to_str().unwrap()],
	);

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let error = manager
		.ensure_worktree("PUB-101", false)
		.expect_err("embedded git checkout should be rejected");

	assert!(
		error
			.to_string()
			.contains("is not a linked git worktree: expected `.git` to be a pointer file")
	);
}

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

#[test]
fn rejects_worktree_removal_when_path_escapes_root_via_parent_components() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let escaped_target = repo_root.join("outside").join("PUB-101");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");
	fs::create_dir_all(&escaped_target).expect("escaped target should exist");

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let escaped_path = worktree_root.join("../outside/PUB-101");
	let error = manager
		.remove_worktree_path(&escaped_path)
		.expect_err("escaped worktree path should be rejected");

	assert!(error.to_string().contains("outside worktree_root"));
	assert!(escaped_target.exists());
}
