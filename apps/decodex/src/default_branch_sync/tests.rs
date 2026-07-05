mod default_branch_git_commands_use_routed_noninteractive_credentials;
mod preflight_repo_root_default_branch_sync_accepts_clean_default_branch_checkout;
mod preflight_repo_root_default_branch_sync_accepts_untracked_decodex_runtime_markers;
mod preflight_repo_root_default_branch_sync_rejects_local_commits_not_on_origin;
mod preflight_repo_root_default_branch_sync_rejects_untracked_overwrite_conflicts;
mod preflight_repo_root_default_branch_sync_rejects_untracked_path_prefix_conflicts;
mod sync_repo_root_default_branch_fast_forwards_local_main;
mod sync_repo_root_default_branch_rejects_non_default_branch_checkout;
mod sync_repo_root_default_branch_rejects_tracked_local_changes;

use std::{
	fs,
	path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::test_support;

fn init_repo() -> (TempDir, PathBuf, PathBuf) {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let repo_root = temp_dir.path().join("repo");
	let remote_root = temp_dir.path().join("origin.git");

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git(
		temp_dir.path(),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path utf-8"),
		],
	);
	run_git(&repo_root, &["init", "--initial-branch", "main"]);
	run_git(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git(&repo_root, &["config", "commit.gpgsign", "false"]);
	run_git(&repo_root, &["config", "tag.gpgsign", "false"]);
	run_git(
		&repo_root,
		&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
	);

	fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

	run_git(&repo_root, &["add", "README.md"]);
	run_git(&repo_root, &["commit", "-m", "seed"]);
	run_git(&repo_root, &["push", "-u", "origin", "main"]);

	(temp_dir, repo_root, remote_root)
}

fn clone_repo(remote_root: &Path, name: &str) -> PathBuf {
	let clone_root = remote_root.parent().expect("remote should have parent").join(name);

	run_git(
		remote_root.parent().expect("remote should have parent"),
		&[
			"clone",
			remote_root.to_str().expect("remote path utf-8"),
			clone_root.to_str().expect("clone path utf-8"),
		],
	);
	run_git(&clone_root, &["config", "user.name", "Decodex Tests"]);
	run_git(&clone_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git(&clone_root, &["config", "commit.gpgsign", "false"]);

	clone_root
}

fn git_stdout(cwd: &Path, args: &[&str]) -> String {
	let output = test_support::hermetic_git_command()
		.arg("-C")
		.arg(cwd)
		.args(args)
		.output()
		.expect("git should run");

	assert!(
		output.status.success(),
		"git {:?} failed in `{}`: {}",
		args,
		cwd.display(),
		String::from_utf8_lossy(&output.stderr)
	);

	String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn run_git(cwd: &Path, args: &[&str]) {
	let status = test_support::hermetic_git_command()
		.arg("-C")
		.arg(cwd)
		.args(args)
		.status()
		.expect("git should run");

	assert!(status.success(), "git {:?} should succeed in `{}`", args, cwd.display());
}
