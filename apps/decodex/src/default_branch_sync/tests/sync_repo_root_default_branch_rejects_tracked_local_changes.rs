use std::fs;

use crate::default_branch_sync::{self, tests};

#[test]
fn sync_repo_root_default_branch_rejects_tracked_local_changes() {
	let (_temp_dir, repo_root, _remote_root) = tests::init_repo();

	fs::write(repo_root.join("README.md"), "dirty\n").expect("tracked change should write");

	let error = default_branch_sync::sync_repo_root_default_branch(&repo_root, "main", None)
		.expect_err("tracked dirty repo root should be rejected");

	assert!(error.to_string().contains("tracked local changes"));
}
