use std::fs;

use crate::default_branch_sync::{
	self,
	tests::{self},
};

#[test]
fn sync_repo_root_default_branch_fast_forwards_local_main() {
	let (_temp_dir, repo_root, remote_root) = tests::init_repo();
	let peer_root = tests::clone_repo(&remote_root, "peer");

	fs::write(peer_root.join("README.md"), "seed\nremote update\n")
		.expect("peer update should write");
	tests::run_git(&peer_root, &["add", "README.md"]);
	tests::run_git(&peer_root, &["commit", "-m", "remote update"]);
	tests::run_git(&peer_root, &["push", "origin", "main"]);

	let before = tests::git_stdout(&repo_root, &["rev-parse", "HEAD"]);

	default_branch_sync::sync_repo_root_default_branch(&repo_root, "main", None)
		.expect("repo root main should fast-forward");

	let after = tests::git_stdout(&repo_root, &["rev-parse", "HEAD"]);
	let remote = tests::git_stdout(&repo_root, &["rev-parse", "refs/remotes/origin/main"]);

	assert_ne!(before, after, "sync should advance local main");
	assert_eq!(after, remote, "local main should match origin/main after sync");
}
