use crate::default_branch_sync::{self, tests};

#[test]
fn preflight_repo_root_default_branch_sync_accepts_clean_default_branch_checkout() {
	let (_temp_dir, repo_root, _remote_root) = tests::init_repo();

	default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
		.expect("clean repo root on the default branch should pass preflight");
}
