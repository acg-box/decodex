use std::fs;

use crate::default_branch_sync::{
	self,
	tests::{self},
};

#[test]
fn preflight_repo_root_default_branch_sync_rejects_local_commits_not_on_origin() {
	let (_temp_dir, repo_root, _remote_root) = tests::init_repo();

	fs::write(repo_root.join("README.md"), "seed\nlocal-only\n")
		.expect("local-only update should write");
	tests::run_git(&repo_root, &["add", "README.md"]);
	tests::run_git(&repo_root, &["commit", "-m", "local only"]);

	let error =
		default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
			.expect_err("local-only commits should block ff-only preflight");

	assert!(error.to_string().contains("cannot fast-forward local `main`"));
	assert!(error.to_string().contains("not on origin"));
}
