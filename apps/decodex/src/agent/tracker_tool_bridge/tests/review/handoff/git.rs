use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{
	agent::{tracker_tool_bridge, tracker_tool_bridge::RepositoryIdentity},
	test_support,
};

#[test]
fn parses_credentialed_https_github_remote() {
	let repository = tracker_tool_bridge::parse_github_repository_identity(
		"https://x-access-token@github.com/hack-ink/decodex.git",
	)
	.expect("credentialed GitHub remote should parse");

	assert_eq!(
		repository,
		RepositoryIdentity { owner: String::from("hack-ink"), name: String::from("decodex") }
	);
}

#[test]
fn parses_default_branch_from_ls_remote_symref_output() {
	let parsed = tracker_tool_bridge::parse_remote_head_symref_output(
		"ref: refs/heads/main\tHEAD\n9c0ffee\tHEAD\n9c0ffee\trefs/heads/main\n",
	);

	assert_eq!(parsed.as_deref(), Some("main"));
}

#[test]
fn ignores_non_head_lines_when_parsing_default_branch_from_ls_remote_output() {
	let parsed = tracker_tool_bridge::parse_remote_head_symref_output(
		"9c0ffee\trefs/heads/main\n9c0ffee\trefs/heads/release/1.x\n",
	);

	assert_eq!(parsed, None);
}

#[test]
fn resolve_lane_default_branch_prefers_cached_origin_head() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_root = temp_dir.path().join("origin.git");
	let repo_root = temp_dir.path().join("repo");

	run_git_for_handoff(
		temp_dir.path(),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path utf-8"),
		],
	);

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git_for_handoff(&repo_root, &["init", "--initial-branch", "main"]);
	run_git_for_handoff(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git_for_handoff(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git_for_handoff(
		&repo_root,
		&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
	);

	fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

	run_git_for_handoff(&repo_root, &["add", "README.md"]);
	run_git_for_handoff(&repo_root, &["commit", "-m", "seed"]);
	run_git_for_handoff(&repo_root, &["push", "-u", "origin", "main"]);
	run_git_for_handoff(&repo_root, &["checkout", "-b", "trunk"]);
	run_git_for_handoff(&repo_root, &["push", "origin", "trunk"]);
	run_git_for_handoff(&remote_root, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
	run_git_for_handoff(
		&repo_root,
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
	run_git_for_handoff(&repo_root, &["checkout", "main"]);

	let resolved = tracker_tool_bridge::resolve_lane_default_branch(&repo_root)
		.expect("default branch should resolve");

	assert_eq!(resolved, "main");
}

#[test]
fn resolve_lane_default_branch_uses_remote_head_when_local_cache_is_missing() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_root = temp_dir.path().join("origin.git");
	let repo_root = temp_dir.path().join("repo");

	run_git_for_handoff(
		temp_dir.path(),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path utf-8"),
		],
	);

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git_for_handoff(&repo_root, &["init", "--initial-branch", "main"]);
	run_git_for_handoff(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git_for_handoff(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git_for_handoff(
		&repo_root,
		&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
	);

	fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

	run_git_for_handoff(&repo_root, &["add", "README.md"]);
	run_git_for_handoff(&repo_root, &["commit", "-m", "seed"]);
	run_git_for_handoff(&repo_root, &["push", "-u", "origin", "main"]);
	run_git_for_handoff(&repo_root, &["checkout", "-b", "trunk"]);
	run_git_for_handoff(&repo_root, &["push", "origin", "trunk"]);
	run_git_for_handoff(&remote_root, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
	run_git_for_handoff(&repo_root, &["checkout", "main"]);

	let resolved = tracker_tool_bridge::resolve_lane_default_branch(&repo_root)
		.expect("default branch should resolve");

	assert_eq!(resolved, "trunk");
}

#[test]
fn resolve_lane_default_branch_uses_cached_origin_head_without_reachable_remote() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let remote_root = temp_dir.path().join("origin.git");
	let repo_root = temp_dir.path().join("repo");

	run_git_for_handoff(
		temp_dir.path(),
		&[
			"init",
			"--bare",
			"--initial-branch",
			"main",
			remote_root.to_str().expect("remote path utf-8"),
		],
	);

	fs::create_dir_all(&repo_root).expect("repo root should exist");

	run_git_for_handoff(&repo_root, &["init", "--initial-branch", "main"]);
	run_git_for_handoff(&repo_root, &["config", "user.name", "Decodex Tests"]);
	run_git_for_handoff(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
	run_git_for_handoff(
		&repo_root,
		&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
	);

	fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

	run_git_for_handoff(&repo_root, &["add", "README.md"]);
	run_git_for_handoff(&repo_root, &["commit", "-m", "seed"]);
	run_git_for_handoff(&repo_root, &["push", "-u", "origin", "main"]);
	run_git_for_handoff(
		&repo_root,
		&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"],
	);
	run_git_for_handoff(
		&repo_root,
		&[
			"remote",
			"set-url",
			"origin",
			temp_dir.path().join("missing-origin.git").to_str().expect("missing remote path utf-8"),
		],
	);

	let resolved = tracker_tool_bridge::resolve_lane_default_branch(&repo_root)
		.expect("default branch should resolve");

	assert_eq!(resolved, "main");
}

fn run_git_for_handoff(cwd: &Path, args: &[&str]) {
	let status = test_support::hermetic_git_command()
		.arg("-C")
		.arg(cwd)
		.args(args)
		.status()
		.expect("git should run");

	assert!(status.success(), "git {:?} should succeed in `{}`", args, cwd.display());
}
