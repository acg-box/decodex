use std::fs;

#[test]
fn merged_worktree_cleanup_debts_detects_dirty_merged_worktree() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("accounts-column-format");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	super::run_git(
		&repo_root,
		&[
			"worktree",
			"add",
			"-b",
			"xy/accounts-column-format",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);

	fs::write(worktree_path.join("README.md"), "feature work\n")
		.expect("worktree file should write");

	super::run_git(&worktree_path, &["add", "README.md"]);
	super::run_git(&worktree_path, &["commit", "-m", "feature work"]);
	super::run_git(
		&repo_root,
		&["merge", "--no-ff", "xy/accounts-column-format", "-m", "land feature"],
	);

	fs::write(worktree_path.join("README.md"), "dirty after land\n")
		.expect("worktree file should become dirty");

	let debts = super::super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
		.expect("cleanup debt scan should succeed");

	assert_eq!(debts.len(), 1);
	assert_eq!(debts[0].branch_name, "xy/accounts-column-format");
	assert_eq!(
		fs::canonicalize(&debts[0].path).expect("debt path should canonicalize"),
		fs::canonicalize(&worktree_path).expect("worktree path should canonicalize")
	);
	assert_eq!(debts[0].cleanliness, super::super::MergedWorktreeCleanliness::Dirty);
}

#[test]
fn merged_worktree_cleanup_debts_treats_decodex_runtime_artifacts_as_clean() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("accounts-column-format");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	super::run_git(
		&repo_root,
		&[
			"worktree",
			"add",
			"-b",
			"xy/accounts-column-format",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);

	fs::write(worktree_path.join("README.md"), "feature work\n")
		.expect("worktree file should write");

	super::run_git(&worktree_path, &["add", "README.md"]);
	super::run_git(&worktree_path, &["commit", "-m", "feature work"]);
	super::run_git(
		&repo_root,
		&["merge", "--no-ff", "xy/accounts-column-format", "-m", "land feature"],
	);

	fs::write(worktree_path.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
		.expect("activity marker should write");

	let control_dir = worktree_path.join(crate::state::RUN_CONTROL_CHANNEL_DIR);

	fs::create_dir_all(&control_dir).expect("run-control directory should create");
	fs::write(control_dir.join("run-1-1.channel"), "channel\n")
		.expect("run-control channel should write");

	let debts = super::super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
		.expect("cleanup debt scan should succeed");

	assert_eq!(debts.len(), 1);
	assert_eq!(debts[0].branch_name, "xy/accounts-column-format");
	assert_eq!(debts[0].cleanliness, super::super::MergedWorktreeCleanliness::Clean);
}

#[test]
fn merged_worktree_cleanup_debts_ignores_dirty_worktree_started_from_old_default() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("scroll-capture-motion");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	super::run_git(
		&repo_root,
		&[
			"worktree",
			"add",
			"-b",
			"xy/scroll-capture-motion",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);

	fs::write(repo_root.join("main.txt"), "main advanced\n")
		.expect("main branch file should write");

	super::run_git(&repo_root, &["add", "main.txt"]);
	super::run_git(&repo_root, &["commit", "-m", "advance main"]);

	fs::write(worktree_path.join("README.md"), "manual dirty work\n")
		.expect("worktree file should become dirty");

	let debts = super::super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
		.expect("cleanup debt scan should succeed");

	assert!(
		debts.is_empty(),
		"dirty worktrees started from an older default commit are manual work, not post-land debt"
	);
}

#[test]
fn merged_worktree_cleanup_debts_ignores_unmerged_worktree() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("dashboard-ws-control-plane");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	super::run_git(
		&repo_root,
		&[
			"worktree",
			"add",
			"-b",
			"xy/dashboard-ws-control-plane",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);

	fs::write(worktree_path.join("README.md"), "feature work\n")
		.expect("worktree file should write");

	super::run_git(&worktree_path, &["add", "README.md"]);
	super::run_git(&worktree_path, &["commit", "-m", "feature work"]);

	let debts = super::super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
		.expect("cleanup debt scan should succeed");

	assert!(debts.is_empty(), "unmerged branch worktrees should remain usable");
}

#[test]
fn merged_worktree_cleanup_debts_ignores_dirty_worktree_at_default_tip() {
	let (_temp_dir, repo_root) = super::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let worktree_path = worktree_root.join("XY-454");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	super::run_git(
		&repo_root,
		&[
			"worktree",
			"add",
			"-b",
			"y/decodex-xy-454",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
			"main",
		],
	);

	fs::write(worktree_path.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "started\n")
		.expect("run activity marker should write");

	let debts = super::super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
		.expect("cleanup debt scan should succeed");

	assert!(debts.is_empty(), "default-tip run worktrees should remain usable");
}
