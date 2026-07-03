use std::{
	ffi::OsString,
	fs,
	io::Error,
	path::PathBuf,
	thread,
	time::{Duration, Instant},
};

use libc::ESRCH;

use crate::{
	state::{RUN_ACTIVITY_MARKER_FILE, RUN_CONTROL_CHANNEL_DIR},
	worktree::{self, WorktreeManager, git, hooks},
};

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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(spec.branch_name, "x/pubfi-pub-101");
	assert!(spec.path.join(".git").is_file());
	assert_eq!(
		super::git_stdout(&spec.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
		"x/pubfi-pub-101"
	);

	let repo_git_dir = fs::canonicalize(repo_root.join(".git")).expect("repo git dir");
	let git_dir = fs::canonicalize(PathBuf::from(super::git_stdout(
		&spec.path,
		&["rev-parse", "--path-format=absolute", "--git-dir"],
	)))
	.expect("git dir should canonicalize");
	let git_common_dir = fs::canonicalize(PathBuf::from(super::git_stdout(
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let hook_log = repo_root.join("after-create.log");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let hook_log = repo_root.join("after-create.log");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let hook_log = repo_root.join("after-create.log");
	let allow_file = repo_root.join("allow-bootstrap");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
		let (_temp_dir, repo_root) = super::init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = super::workspace_hooks(
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
		let (_temp_dir, repo_root) = super::init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
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
	let (_temp_dir, repo_root) = super::init_repo();
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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

#[test]
fn creates_linked_worktree_when_repo_root_is_also_a_linked_worktree() {
	let (_temp_dir, primary_repo_root) = super::init_repo();
	let linked_repo_root = primary_repo_root.parent().unwrap().join("linked-root");

	super::run_git(
		&primary_repo_root,
		&["worktree", "add", "--quiet", "--detach", linked_repo_root.to_str().unwrap(), "HEAD"],
	);
	super::run_git(
		&linked_repo_root,
		&["checkout", "--quiet", "-B", "x/pubfi-linked-root", "HEAD"],
	);

	let worktree_root = linked_repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &linked_repo_root, &worktree_root);
	let spec = manager
		.ensure_worktree("PUB-101", false)
		.expect("worktree should be created from linked repo root");

	assert_eq!(spec.branch_name, "x/pubfi-pub-101");
	assert!(spec.path.join(".git").is_file());

	let repo_git_dir = fs::canonicalize(PathBuf::from(super::git_stdout(
		&linked_repo_root,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
	)))
	.expect("linked repo common dir should canonicalize");
	let git_dir = fs::canonicalize(PathBuf::from(super::git_stdout(
		&spec.path,
		&["rev-parse", "--path-format=absolute", "--git-dir"],
	)))
	.expect("git dir should canonicalize");
	let git_common_dir = fs::canonicalize(PathBuf::from(super::git_stdout(
		&spec.path,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
	)))
	.expect("git common dir should canonicalize");

	assert!(git_dir.starts_with(repo_git_dir.join("worktrees")));
	assert_eq!(git_common_dir, repo_git_dir);
}

#[test]
fn linked_worktree_inherits_repo_local_identity_config() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	super::run_git(&repo_root, &["config", "user.signingkey", "worktree-tests"]);
	super::run_git(&repo_root, &["config", "codex.github-identity", "y"]);
	super::run_git(&repo_root, &["config", "codex.linear-workspace", "hackink"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(super::git_stdout(&spec.path, &["config", "--get", "user.name"]), "Decodex Tests");
	assert_eq!(
		super::git_stdout(&spec.path, &["config", "--get", "user.email"]),
		"decodex-tests@example.com"
	);
	assert_eq!(super::git_stdout(&spec.path, &["config", "--get", "commit.gpgsign"]), "false");
	assert_eq!(
		super::git_stdout(&spec.path, &["config", "--get", "user.signingkey"]),
		"worktree-tests"
	);
	assert_eq!(super::git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
	assert_eq!(
		super::git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
		"hackink"
	);
}

#[test]
fn linked_worktree_inherits_repo_local_identity_from_included_config() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let included_config = repo_root.parent().unwrap().join("identity.inc");

	super::run_git(&repo_root, &["config", "--unset-all", "user.name"]);
	super::run_git(&repo_root, &["config", "--unset-all", "user.email"]);
	fs::write(
			&included_config,
			"[user]\n\tname = Included Tests\n\temail = included@example.com\n[codex]\n\tgithub-identity = y\n\tlinear-workspace = hackink\n",
			)
			.expect("included config should write");
	super::run_git(
		&repo_root,
		&["config", "--local", "include.path", included_config.to_str().unwrap()],
	);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(super::git_stdout(&spec.path, &["config", "--get", "user.name"]), "Included Tests");
	assert_eq!(
		super::git_stdout(&spec.path, &["config", "--get", "user.email"]),
		"included@example.com"
	);
	assert_eq!(super::git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
	assert_eq!(
		super::git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
		"hackink"
	);
}

#[test]
fn linked_worktree_uses_existing_remote_lane_branch_when_present() {
	let (_temp_dir, repo_root) = super::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let lane_branch = "x/pubfi-pub-101";

	super::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	super::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	super::run_git(&repo_root, &["push", "-u", "origin", "main"]);
	super::run_git(&repo_root, &["checkout", "-b", lane_branch]);
	fs::write(repo_root.join("LANE.md"), "lane branch\n").expect("lane file should write");
	super::run_git(&repo_root, &["add", "LANE.md"]);
	super::run_git(&repo_root, &["commit", "-m", "lane branch"]);
	super::run_git(&repo_root, &["push", "-u", "origin", lane_branch]);
	super::run_git(&repo_root, &["checkout", "main"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(super::git_stdout(&spec.path, &["rev-parse", "--abbrev-ref", "HEAD"]), lane_branch);
	assert_eq!(
		fs::read_to_string(spec.path.join("LANE.md")).expect("lane file should exist"),
		"lane branch\n"
	);
	assert_eq!(
		super::git_stdout(&spec.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}

#[test]
fn linked_worktree_push_uses_normalized_absolute_origin_when_source_remote_is_relative() {
	let (_temp_dir, repo_root) = super::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	super::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	super::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	super::run_git(&repo_root, &["push", "-u", "origin", "main"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	fs::write(spec.path.join("WORKTREE.md"), "linked worktree lane\n")
		.expect("worktree file should write");
	super::run_git(&spec.path, &["add", "WORKTREE.md"]);
	super::run_git(&spec.path, &["commit", "-m", "worktree change"]);
	super::run_git(&spec.path, &["push", "-u", "origin", "x/pubfi-pub-101"]);

	assert_eq!(
		super::git_stdout(&spec.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}

#[test]
fn reused_linked_worktree_normalizes_relative_origin_on_reentry() {
	let (_temp_dir, repo_root) = super::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	super::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	super::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	super::run_git(&repo_root, &["push", "-u", "origin", "main"]);

	let created = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	super::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);

	let reused = manager.ensure_worktree("PUB-101", false).expect("worktree should be reused");

	assert!(reused.reused_existing);
	assert_eq!(reused.path, created.path);
	assert_eq!(
		super::git_stdout(&reused.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}

#[test]
fn linked_worktree_leaves_home_relative_origin_unchanged() {
	let (_temp_dir, repo_root) = super::init_repo();

	super::run_git(&repo_root, &["remote", "set-url", "origin", "~/lane-remote.git"]);
	git::normalize_origin_remote_for_worktrees(&repo_root)
		.expect("home-relative remotes should bypass normalization");

	assert_eq!(
		super::git_stdout(&repo_root, &["remote", "get-url", "origin"]),
		"~/lane-remote.git"
	);
	assert!(!worktree::is_relative_filesystem_remote("~/lane-remote.git"));
	assert!(!worktree::is_relative_filesystem_remote("~"));
}

#[test]
fn linked_worktree_rolls_back_when_origin_normalization_fails() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let spec = manager.plan_for_issue("PUB-101");

	super::run_git(&repo_root, &["remote", "set-url", "origin", "../missing-remote.git"]);

	let error = manager
		.ensure_worktree("PUB-101", false)
		.expect_err("worktree creation should fail when origin normalization fails");

	assert!(
		error.to_string().contains("No such file or directory")
			|| error.to_string().contains("does not exist"),
		"unexpected error: {error:?}"
	);
	assert!(!spec.path.exists(), "failed setup should remove the new worktree path");
	assert!(
		!git::worktree_is_registered(&repo_root, &spec.path)
			.expect("worktree registration should inspect"),
		"failed setup should unregister the new worktree"
	);
}

#[test]
fn linked_worktree_fails_when_remote_branch_probe_errors() {
	let (temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let missing_remote = temp_dir.path().join("missing-origin.git");

	super::run_git(&repo_root, &["remote", "set-url", "origin", missing_remote.to_str().unwrap()]);

	let error = manager
		.ensure_worktree("PUB-101", false)
		.expect_err("worktree create should fail when remote probe errors");

	assert!(error.to_string().contains("Failed to inspect remote worktree branch"));
}

#[test]
fn rejects_reused_non_worktree_checkout_with_embedded_git_dir() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("PUB-101");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");
	super::run_git(
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should exist");

	assert!(manager.remove_worktree_path(&spec.path).expect("worktree should remove"));
	assert!(!spec.path.exists());
	assert!(
		!super::git_stdout(&repo_root, &["worktree", "list", "--porcelain"])
			.contains(&format!("worktree {}", spec.path.display()))
	);
}

#[test]
fn removes_orphaned_marker_directory_without_linked_git_metadata() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let orphan_path = worktree_root.join("PUB-101");
	let hook_log = repo_root.join("before-remove.log");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let hook_log = repo_root.join("before-remove.log");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let rogue_path = worktree_root.join("PUB-rogue");
	let hook_log = repo_root.join("before-remove.log");

	fs::create_dir_all(&rogue_path).expect("rogue path should exist");
	fs::write(rogue_path.join(".git"), b"not-a-worktree\n")
		.expect("rogue path should contain a fake git pointer");

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let hooks = super::workspace_hooks(
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
	let (_temp_dir, repo_root) = super::init_repo();
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
