use std::fs;

use crate::default_branch_sync::{
	self,
	tests::{self},
};

#[test]
fn preflight_repo_root_default_branch_sync_rejects_untracked_overwrite_conflicts() {
	let (_temp_dir, repo_root, remote_root) = tests::init_repo();
	let peer_root = tests::clone_repo(&remote_root, "peer");

	fs::write(peer_root.join("conflict.txt"), "remote tracked file\n")
		.expect("peer conflict file should write");
	tests::run_git(&peer_root, &["add", "conflict.txt"]);
	tests::run_git(&peer_root, &["commit", "-m", "add conflict file"]);
	tests::run_git(&peer_root, &["push", "origin", "main"]);
	fs::write(repo_root.join("conflict.txt"), "local untracked file\n")
		.expect("repo-root untracked conflict file should write");

	let error =
		default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
			.expect_err("incoming tracked paths must not overwrite local untracked files");

	assert!(error.to_string().contains("untracked local files"));
	assert!(error.to_string().contains("conflict.txt"));
}
