use std::{
	ffi::OsString,
	fs,
	io::Error,
	path::PathBuf,
	thread,
	time::{Duration, Instant},
};

use libc::ESRCH;

use crate::worktree::{self, WorktreeManager, git, hooks, tests};
#[test]
fn workspace_hook_shell_uses_posix_sh_for_sh_or_missing_shell() {
	for shell_env in [Some(OsString::from("/bin/sh")), None] {
		let (shell, shell_flag) = worktree::workspace_hook_shell_from_env(shell_env);

		assert_eq!(shell, std::ffi::OsString::from("/bin/sh"));
		assert_eq!(shell_flag, "-c");
	}
}

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

#[test]
fn workspace_hook_command_returns_without_waiting_for_background_child_pipe_close() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let start = Instant::now();
	let output = hooks::run_workspace_hook_shell_command(
		"sleep 5 & printf 'done\\n'",
		&repo_root,
		&[],
		Duration::from_secs(1),
	)
	.expect("shell exit should not block on inherited stdout/stderr pipe handles");

	assert!(output.status.success(), "backgrounded child should not fail the shell command");
	assert_eq!(String::from_utf8_lossy(&output.stdout), "done\n");
	assert!(
		start.elapsed() < Duration::from_secs(3),
		"hook output collection should not wait for background child pipe closure after shell exit"
	);
}

#[cfg(unix)]
#[test]
fn workspace_hook_timeout_kills_background_descendants() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let child_pid_file = repo_root.join("hook-child.pid");
	let error = hooks::run_workspace_hook_shell_command(
		"sleep 300 & bg=$!; printf '%s\n' \"$bg\" > \"$DECODEX_REPO_ROOT/hook-child.pid\"; wait",
		&repo_root,
		&[("DECODEX_REPO_ROOT", repo_root.display().to_string())],
		Duration::from_secs(1),
	)
	.expect_err("timed out hook should fail");

	assert!(error.to_string().contains("exceeded the 1s timeout"));

	let child_pid = fs::read_to_string(&child_pid_file)
		.expect("background child pid should be recorded before timeout")
		.trim()
		.parse::<i32>()
		.expect("background child pid should parse");
	let kill_deadline = Instant::now() + Duration::from_secs(2);

	while process_is_alive(child_pid) && Instant::now() < kill_deadline {
		thread::sleep(Duration::from_millis(25));
	}

	assert!(
		!process_is_alive(child_pid),
		"timed out workspace hook should terminate background descendants"
	);
}

#[cfg(unix)]
fn process_is_alive(process_id: i32) -> bool {
	let result = unsafe { libc::kill(process_id, 0) };

	if result == 0 {
		return true;
	}

	Error::last_os_error().raw_os_error() != Some(ESRCH)
}

#[test]
fn after_create_hook_tolerates_verbose_success_output() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = tests::workspace_hooks(
		r#"
[execution.workspace_hooks]
timeout_seconds = 1
after_create_commands = ["yes hook-output | head -c 131072 >/dev/stdout"]
before_remove_commands = []
			"#,
	);
	let spec = manager
		.ensure_worktree_with_hooks("PUB-101", false, &hooks)
		.expect("verbose successful hook should not deadlock on captured output");

	assert!(spec.path.exists(), "worktree should remain usable after verbose bootstrap");
	assert!(
		!hooks::after_create_pending_marker_path(&spec.path).exists(),
		"successful verbose hook should clear the pending marker"
	);
}
