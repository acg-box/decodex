use std::fs;

use crate::{
	default_branch_sync::{self, tests},
	state::RUN_ACTIVITY_MARKER_FILE,
};

#[test]
fn accepts_untracked_runtime_markers() {
	let (_temp_dir, repo_root, _remote_root) = tests::init_repo();

	fs::write(repo_root.join(RUN_ACTIVITY_MARKER_FILE), "runtime marker\n")
		.expect("runtime marker should write");
	default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
		.expect("untracked Decodex activity marker should not block clean-source preflight");
}
