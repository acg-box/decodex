use std::{
	fs,
	io::Error,
	thread,
	time::{Duration, Instant},
};

use libc::ESRCH;

use crate::worktree::{WorktreeManager, hooks, tests};

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
