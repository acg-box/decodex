use std::fs;

use crate::default_branch_sync::{
	self,
	tests::{self},
};

#[test]
fn preflight_repo_root_default_branch_sync_rejects_untracked_path_prefix_conflicts() {
	let (_temp_dir, repo_root, remote_root) = tests::init_repo();
	let peer_root = tests::clone_repo(&remote_root, "peer");

	fs::create_dir_all(peer_root.join("openwiki")).expect("peer nested directory should exist");
	fs::write(peer_root.join("openwiki/guide.md"), "remote tracked file\n")
		.expect("peer nested file should write");
	tests::run_git(&peer_root, &["add", "openwiki/guide.md"]);
	tests::run_git(&peer_root, &["commit", "-m", "add nested file"]);
	tests::run_git(&peer_root, &["push", "origin", "main"]);
	fs::write(repo_root.join("openwiki"), "local untracked file\n")
		.expect("repo-root conflicting untracked file should write");

	let error =
		default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
			.expect_err("incoming tracked directories must not overwrite local untracked files");

	assert!(error.to_string().contains("untracked local files"));
	assert!(error.to_string().contains("openwiki"));
}
