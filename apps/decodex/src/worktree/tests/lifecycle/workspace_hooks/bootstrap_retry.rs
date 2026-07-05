use std::fs;

use crate::worktree::{WorktreeManager, hooks, tests};

#[test]
fn reused_lane_retries_bootstrap_after_interrupted_create_window() {
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
	let planned = manager.plan_for_issue("PUB-101");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	manager
		.create_linked_worktree(&planned, Some(&hooks))
		.expect("linked worktree should be created");
	manager.validate_worktree_boundary(&planned.path).expect("created worktree should validate");

	assert!(
		hooks::after_create_pending_marker_path(&planned.path).exists(),
		"newly created lane should persist the pending bootstrap marker before first hook run"
	);
	assert!(!hook_log.exists(), "simulated crash window should not have run hooks yet");

	let reused = manager
		.ensure_worktree_with_hooks("PUB-101", false, &hooks)
		.expect("reused lane should resume interrupted bootstrap");

	assert!(reused.reused_existing);
	assert_eq!(
		fs::read_to_string(&hook_log).expect("hook log should exist after resumed bootstrap"),
		"x/pubfi-pub-101\n"
	);
	assert!(
		!hooks::after_create_pending_marker_path(&planned.path).exists(),
		"successful resumed bootstrap should clear the pending marker"
	);
}

#[test]
fn after_create_hook_retries_before_reused_lane_dispatch() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let hook_log = repo_root.join("after-create.log");
	let allow_file = repo_root.join("allow-bootstrap");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
after_create_commands = ["printf '%s\n' \"$DECODEX_BRANCH\" >> \"$DECODEX_REPO_ROOT/after-create.log\" && test -f \"$DECODEX_REPO_ROOT/allow-bootstrap\""]
before_remove_commands = []
timeout_seconds = 60
			"#,
	);
	let planned = manager.plan_for_issue("PUB-101");
	let first_error = manager
		.ensure_worktree_with_hooks("PUB-101", false, &hooks)
		.expect_err("missing bootstrap prerequisite should fail");

	assert!(first_error.to_string().contains("Workspace hook `after_create` command"));
	assert_eq!(
		fs::read_to_string(&hook_log).expect("hook log should exist after first failure"),
		"x/pubfi-pub-101\n"
	);
	assert!(
		hooks::after_create_pending_marker_path(&planned.path).exists(),
		"failed bootstrap should leave the pending marker behind"
	);

	fs::write(&allow_file, "ready\n").expect("bootstrap prerequisite should write");

	let reused = manager
		.ensure_worktree_with_hooks("PUB-101", false, &hooks)
		.expect("reused lane should rerun the pending bootstrap hook");

	assert!(reused.reused_existing);
	assert_eq!(
		fs::read_to_string(&hook_log).expect("hook log should include retried bootstrap"),
		"x/pubfi-pub-101\nx/pubfi-pub-101\n"
	);
	assert!(
		!hooks::after_create_pending_marker_path(&planned.path).exists(),
		"successful retry should clear the pending bootstrap marker"
	);
}

#[test]
fn after_create_hook_handles_hook_managed_pending_marker_removal() {
	{
		let (_temp_dir, repo_root) = tests::init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = tests::workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = ["rm -f \"$DECODEX_WORKTREE_PATH/.decodex-after-create.pending\""]
before_remove_commands = []
timeout_seconds = 60
			"#,
		);
		let spec = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect("successful hook that removes the marker should still pass");

		assert!(spec.path.exists(), "worktree should remain usable after bootstrap");
		assert!(
			!hooks::after_create_pending_marker_path(&spec.path).exists(),
			"successful hook should not leave a stale pending marker behind"
		);
	}
	{
		let (_temp_dir, repo_root) = tests::init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = tests::workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = ["rm -f \"$DECODEX_WORKTREE_PATH/.decodex-after-create.pending\"", "exit 23"]
before_remove_commands = []
timeout_seconds = 60
			"#,
		);
		let planned = manager.plan_for_issue("PUB-101");
		let error = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect_err("failed hook should still leave the lane pending for retry");

		assert!(
			error.to_string().contains("Workspace hook `after_create` command `exit 23` failed")
		);
		assert!(
			hooks::after_create_pending_marker_path(&planned.path).exists(),
			"failed bootstrap should restore the pending marker even if an earlier command removed it"
		);
	}
}
