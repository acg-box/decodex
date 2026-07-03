use std::{fs, path::PathBuf};

use crate::worktree::{self, WorktreeManager, git, tests};
#[test]
fn creates_linked_worktree_when_repo_root_is_also_a_linked_worktree() {
	let (_temp_dir, primary_repo_root) = tests::init_repo();
	let linked_repo_root = primary_repo_root.parent().unwrap().join("linked-root");

	tests::run_git(
		&primary_repo_root,
		&["worktree", "add", "--quiet", "--detach", linked_repo_root.to_str().unwrap(), "HEAD"],
	);
	tests::run_git(
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

	let repo_git_dir = fs::canonicalize(PathBuf::from(tests::git_stdout(
		&linked_repo_root,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
	)))
	.expect("linked repo common dir should canonicalize");
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
}

#[test]
fn linked_worktree_inherits_repo_local_identity_config() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	tests::run_git(&repo_root, &["config", "user.signingkey", "worktree-tests"]);
	tests::run_git(&repo_root, &["config", "codex.github-identity", "y"]);
	tests::run_git(&repo_root, &["config", "codex.linear-workspace", "hackink"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "user.name"]), "Decodex Tests");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "user.email"]),
		"decodex-tests@example.com"
	);
	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "commit.gpgsign"]), "false");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "user.signingkey"]),
		"worktree-tests"
	);
	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
		"hackink"
	);
}

#[test]
fn linked_worktree_inherits_repo_local_identity_from_included_config() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let included_config = repo_root.parent().unwrap().join("identity.inc");

	tests::run_git(&repo_root, &["config", "--unset-all", "user.name"]);
	tests::run_git(&repo_root, &["config", "--unset-all", "user.email"]);
	fs::write(
			&included_config,
			"[user]\n\tname = Included Tests\n\temail = included@example.com\n[codex]\n\tgithub-identity = y\n\tlinear-workspace = hackink\n",
			)
			.expect("included config should write");
	tests::run_git(
		&repo_root,
		&["config", "--local", "include.path", included_config.to_str().unwrap()],
	);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "user.name"]), "Included Tests");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "user.email"]),
		"included@example.com"
	);
	assert_eq!(tests::git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
	assert_eq!(
		tests::git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
		"hackink"
	);
}

#[test]
fn linked_worktree_uses_existing_remote_lane_branch_when_present() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let lane_branch = "x/pubfi-pub-101";

	tests::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	tests::run_git(&repo_root, &["push", "-u", "origin", "main"]);
	tests::run_git(&repo_root, &["checkout", "-b", lane_branch]);
	fs::write(repo_root.join("LANE.md"), "lane branch\n").expect("lane file should write");
	tests::run_git(&repo_root, &["add", "LANE.md"]);
	tests::run_git(&repo_root, &["commit", "-m", "lane branch"]);
	tests::run_git(&repo_root, &["push", "-u", "origin", lane_branch]);
	tests::run_git(&repo_root, &["checkout", "main"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	assert_eq!(tests::git_stdout(&spec.path, &["rev-parse", "--abbrev-ref", "HEAD"]), lane_branch);
	assert_eq!(
		fs::read_to_string(spec.path.join("LANE.md")).expect("lane file should exist"),
		"lane branch\n"
	);
	assert_eq!(
		tests::git_stdout(&spec.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}

#[test]
fn linked_worktree_push_uses_normalized_absolute_origin_when_source_remote_is_relative() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	tests::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	tests::run_git(&repo_root, &["push", "-u", "origin", "main"]);

	let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	fs::write(spec.path.join("WORKTREE.md"), "linked worktree lane\n")
		.expect("worktree file should write");
	tests::run_git(&spec.path, &["add", "WORKTREE.md"]);
	tests::run_git(&spec.path, &["commit", "-m", "worktree change"]);
	tests::run_git(&spec.path, &["push", "-u", "origin", "x/pubfi-pub-101"]);

	assert_eq!(
		tests::git_stdout(&spec.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}

#[test]
fn reused_linked_worktree_normalizes_relative_origin_on_reentry() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

	tests::run_git(
		bare_remote.parent().unwrap(),
		&["init", "--bare", bare_remote.to_str().unwrap()],
	);
	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
	tests::run_git(&repo_root, &["push", "-u", "origin", "main"]);

	let created = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);

	let reused = manager.ensure_worktree("PUB-101", false).expect("worktree should be reused");

	assert!(reused.reused_existing);
	assert_eq!(reused.path, created.path);
	assert_eq!(
		tests::git_stdout(&reused.path, &["remote", "get-url", "origin"]),
		fs::canonicalize(&bare_remote)
			.expect("bare remote should canonicalize")
			.to_str()
			.expect("bare remote should be valid UTF-8")
	);
}

#[test]
fn linked_worktree_leaves_home_relative_origin_unchanged() {
	let (_temp_dir, repo_root) = tests::init_repo();

	tests::run_git(&repo_root, &["remote", "set-url", "origin", "~/lane-remote.git"]);
	git::normalize_origin_remote_for_worktrees(&repo_root)
		.expect("home-relative remotes should bypass normalization");

	assert_eq!(
		tests::git_stdout(&repo_root, &["remote", "get-url", "origin"]),
		"~/lane-remote.git"
	);
	assert!(!worktree::is_relative_filesystem_remote("~/lane-remote.git"));
	assert!(!worktree::is_relative_filesystem_remote("~"));
}

#[test]
fn linked_worktree_rolls_back_when_origin_normalization_fails() {
	let (_temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let spec = manager.plan_for_issue("PUB-101");

	tests::run_git(&repo_root, &["remote", "set-url", "origin", "../missing-remote.git"]);

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
	let (temp_dir, repo_root) = tests::init_repo();
	let worktree_root = repo_root.join(".worktrees");
	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let missing_remote = temp_dir.path().join("missing-origin.git");

	tests::run_git(&repo_root, &["remote", "set-url", "origin", missing_remote.to_str().unwrap()]);

	let error = manager
		.ensure_worktree("PUB-101", false)
		.expect_err("worktree create should fail when remote probe errors");

	assert!(error.to_string().contains("Failed to inspect remote worktree branch"));
}
