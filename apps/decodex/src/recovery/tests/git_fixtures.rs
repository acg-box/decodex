use std::{fs, path::Path};

use tempfile::TempDir;

use crate::test_support;

pub(in crate::recovery::tests) fn init_git_repo(path: &Path) {
	fs::create_dir_all(path).expect("git repo path should create");

	let status = test_support::hermetic_git_command()
		.arg("-C")
		.arg(path)
		.arg("init")
		.status()
		.expect("git init should run");

	assert!(status.success(), "git init should succeed");
}

pub(in crate::recovery::tests) fn commit_test_file(
	path: &Path,
	file_name: &str,
	body: &str,
	message: &str,
) {
	fs::write(path.join(file_name), body).expect("test file should write");

	run_git(path, &["add", file_name]);
	run_git(
		path,
		&[
			"-c",
			"user.name=Decodex Test",
			"-c",
			"user.email=decodex-test@example.invalid",
			"-c",
			"commit.gpgsign=false",
			"commit",
			"-m",
			message,
		],
	);
}

pub(in crate::recovery::tests) fn init_clean_git_repo_with_remote_default(
	path: &Path,
	branch_name: &str,
) {
	init_git_repo(path);
	run_git(path, &["checkout", "-B", "main"]);
	commit_test_file(path, "README.md", "base\n", "base");
	run_git(path, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
	run_git(path, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
	run_git(path, &["checkout", "-B", branch_name]);
}

pub(in crate::recovery::tests) fn run_git(repo: &Path, args: &[&str]) -> String {
	let output = test_support::hermetic_git_command()
		.arg("-C")
		.arg(repo)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {:?} failed: {}",
		args,
		String::from_utf8_lossy(&output.stderr)
	);

	String::from_utf8(output.stdout).expect("git stdout should be utf8").trim().to_owned()
}

pub(in crate::recovery::tests) fn temp_git_worktree(
	branch_name: &str,
) -> (TempDir, String, String) {
	let temp_dir = TempDir::new().expect("temp git repo should exist");
	let repo = temp_dir.path();

	run_git(repo, &["init"]);
	run_git(repo, &["config", "user.email", "decodex@example.invalid"]);
	run_git(repo, &["config", "user.name", "Decodex Test"]);
	run_git(repo, &["checkout", "-b", branch_name]);

	let first_head = commit_file(repo, "first\n");
	let second_head = commit_file(repo, "second\n");

	(temp_dir, first_head, second_head)
}

pub(in crate::recovery::tests) fn temp_rebased_git_worktree(
	branch_name: &str,
) -> (TempDir, String, String) {
	let (temp_dir, first_head, _) = temp_git_worktree(branch_name);
	let repo = temp_dir.path();

	run_git(repo, &["checkout", "--orphan", "rebased"]);
	run_git(repo, &["rm", "-rf", "."]);

	let rebased_head = commit_file(repo, "rebased\n");

	run_git(repo, &["branch", "-D", branch_name]);
	run_git(repo, &["branch", "-m", branch_name]);

	(temp_dir, first_head, rebased_head)
}

fn commit_file(repo: &Path, contents: &str) -> String {
	fs::write(repo.join("tracked.txt"), contents).expect("tracked file should write");

	run_git(repo, &["add", "tracked.txt"]);
	run_git(repo, &["commit", "-m", "test commit"]);

	run_git(repo, &["rev-parse", "HEAD"])
}
