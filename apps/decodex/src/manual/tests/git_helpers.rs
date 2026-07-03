use std::{
	fs,
	path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::{
	manual, manual::tests::fixtures::MergedManualLandBranch, test_support,
	worktree::WorktreeManager,
};

pub(in crate::manual::tests) fn init_git_checkout(
	temp_dir: &TempDir,
	directory_name: &str,
) -> PathBuf {
	let checkout = temp_dir.path().join(directory_name);

	assert!(
		test_support::hermetic_git_command()
			.args(["init", "-b", "main"])
			.current_dir(temp_dir.path())
			.arg(&checkout)
			.status()
			.expect("git init should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.name", "Decodex Tests"])
			.current_dir(&checkout)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.email", "decodex-tests@example.com"])
			.current_dir(&checkout)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "commit.gpgsign", "false"])
			.current_dir(&checkout)
			.status()
			.expect("git config should run")
			.success()
	);

	checkout
}

pub(in crate::manual::tests) fn git_success(cwd: &Path, args: &[&str]) {
	assert!(
		test_support::hermetic_git_command()
			.args(args)
			.current_dir(cwd)
			.status()
			.expect("git command should run")
			.success(),
		"git {:?} should succeed",
		args
	);
}

pub(in crate::manual::tests) fn git_add_and_commit(cwd: &Path, pathspec: &str, message: &str) {
	assert!(
		test_support::hermetic_git_command()
			.args(["add", pathspec])
			.current_dir(cwd)
			.status()
			.expect("git add should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["commit", "-m", message])
			.current_dir(cwd)
			.status()
			.expect("git commit should run")
			.success()
	);
}

pub(in crate::manual::tests) fn init_git_checkout_with_origin(temp_dir: &TempDir) -> PathBuf {
	let remote_root = temp_dir.path().join("origin.git");
	let checkout = temp_dir.path().join("repo");

	assert!(
		test_support::hermetic_git_command()
			.args(["init", "--bare", "--initial-branch", "main"])
			.arg(&remote_root)
			.status()
			.expect("bare origin should init")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["clone"])
			.arg(&remote_root)
			.arg(&checkout)
			.status()
			.expect("repo should clone")
			.success()
	);

	git_success(&checkout, &["config", "user.name", "Decodex Tests"]);
	git_success(&checkout, &["config", "user.email", "decodex-tests@example.com"]);
	git_success(&checkout, &["config", "commit.gpgsign", "false"]);

	fs::write(checkout.join("README.md"), "bootstrap\n").expect("readme should write");

	git_add_and_commit(&checkout, "README.md", "bootstrap repo");
	git_success(&checkout, &["push", "origin", "main"]);

	checkout
}

pub(in crate::manual::tests) fn merge_manual_land_test_branch(
	repo_root: &Path,
	worktree_root: &Path,
) -> MergedManualLandBranch {
	let worktree_manager = WorktreeManager::new("decodex", repo_root, worktree_root);
	let worktree = worktree_manager
		.ensure_worktree("manual-land-cleanup", false)
		.expect("manual land worktree should create");

	fs::write(worktree.path.join("feature.txt"), "manual land\n")
		.expect("feature file should write");

	git_add_and_commit(&worktree.path, "feature.txt", "manual land feature");

	let head_oid = manual::run_git_capture(&worktree.path, &["rev-parse", "HEAD"])
		.expect("PR head should read");

	git_success(repo_root, &["merge", "--no-ff", &worktree.branch_name, "-m", "land feature"]);

	let merge_commit =
		manual::run_git_capture(repo_root, &["rev-parse", "HEAD"]).expect("merge head");

	git_success(repo_root, &["push", "origin", "main"]);

	MergedManualLandBranch {
		branch_name: worktree.branch_name,
		head_oid,
		merge_commit,
		worktree_path: worktree.path,
	}
}

pub(in crate::manual::tests) fn remove_test_lane_checkout(
	repo_root: &Path,
	worktree_path: &Path,
	branch_name: &str,
) {
	git_success(worktree_path, &["checkout", "--detach"]);
	git_success(repo_root, &["branch", "-D", branch_name]);
	git_success(
		repo_root,
		&[
			"worktree",
			"remove",
			"--force",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
		],
	);
}

pub(in crate::manual::tests) fn create_dirty_merged_worktree_debt(
	repo_root: &Path,
	worktree_root: &Path,
) {
	let worktree_manager = WorktreeManager::new("decodex", repo_root, worktree_root);
	let worktree =
		worktree_manager.ensure_worktree("XY-999", false).expect("debt worktree should create");

	fs::write(worktree.path.join("debt.txt"), "debt\n").expect("debt file should write");

	git_add_and_commit(&worktree.path, "debt.txt", "debt feature");
	git_success(repo_root, &["merge", "--no-ff", &worktree.branch_name, "-m", "land debt"]);
	git_success(repo_root, &["push", "origin", "main"]);

	fs::write(worktree.path.join("debt.txt"), "dirty debt\n")
		.expect("debt worktree should become dirty");
}
