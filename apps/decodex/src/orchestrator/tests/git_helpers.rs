use crate::{
	orchestrator::tests::{Path, fs},
	test_support,
};

pub(super) fn git_output(worktree_path: &Path, args: &[&str]) -> String {
	let output = test_support::hermetic_git_command()
		.arg("-C")
		.arg(worktree_path)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {} should succeed: {}",
		args.join(" "),
		String::from_utf8_lossy(&output.stderr),
	);

	String::from_utf8(output.stdout).expect("git output should be utf-8").trim().to_owned()
}

pub(super) fn git_status_success(worktree_path: &Path, args: &[&str]) {
	let output = test_support::hermetic_git_command()
		.arg("-C")
		.arg(worktree_path)
		.args(args)
		.output()
		.expect("git command should run");

	assert!(
		output.status.success(),
		"git {} should succeed: {}",
		args.join(" "),
		String::from_utf8_lossy(&output.stderr),
	);
}

pub(super) fn commit_worktree_change(
	worktree_path: &Path,
	file_name: &str,
	contents: &str,
	message: &str,
) -> String {
	git_status_success(worktree_path, &["config", "user.name", "Decodex Tests"]);
	git_status_success(worktree_path, &["config", "user.email", "decodex-tests@example.com"]);

	let absolute_path = worktree_path.join(file_name);

	if let Some(parent) = absolute_path.parent() {
		fs::create_dir_all(parent).expect("worktree file parent should exist");
	}

	fs::write(absolute_path, contents).expect("worktree file should write");

	git_status_success(worktree_path, &["add", file_name]);
	git_status_success(worktree_path, &["commit", "-m", message]);

	git_output(worktree_path, &["rev-parse", "HEAD"])
}

pub(super) fn try_git_local_config_value(repo_root: &Path, key: &str) -> Option<String> {
	let output = test_support::hermetic_git_command()
		.arg("-C")
		.arg(repo_root)
		.args(["config", "--local", "--get", key])
		.output()
		.expect("git config should run");

	if !output.status.success() {
		return None;
	}

	Some(
		String::from_utf8(output.stdout)
			.expect("git config output should be utf-8")
			.trim()
			.to_owned(),
	)
}

pub(super) fn git_remote_url(repo_root: &Path, remote_name: &str) -> Option<String> {
	let output = test_support::hermetic_git_command()
		.arg("-C")
		.arg(repo_root)
		.args(["remote", "get-url", remote_name])
		.output()
		.expect("git remote get-url should run");

	if !output.status.success() {
		return None;
	}

	Some(
		String::from_utf8(output.stdout)
			.expect("git remote get-url output should be utf-8")
			.trim()
			.to_owned(),
	)
}

pub(super) fn add_origin_remote(repo_root: &Path, remote_root: &Path) {
	let remote_url = remote_root.display().to_string();

	git_status_success(
		remote_root.parent().expect("remote root should have parent"),
		&[
			"init",
			"--bare",
			"-b",
			"main",
			remote_root.to_str().expect("remote path should be utf-8"),
		],
	);
	git_status_success(repo_root, &["remote", "add", "origin", remote_url.as_str()]);
	git_status_success(repo_root, &["push", "-u", "origin", "main"]);
}

pub(super) fn checkout_new_branch(repo_root: &Path, branch_name: &str) {
	git_status_success(repo_root, &["checkout", "-b", branch_name]);
}
