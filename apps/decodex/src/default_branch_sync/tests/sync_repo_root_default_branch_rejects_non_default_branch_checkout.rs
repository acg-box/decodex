use crate::default_branch_sync::{
	self,
	tests::{self},
};

#[test]
fn sync_repo_root_default_branch_rejects_non_default_branch_checkout() {
	let (_temp_dir, repo_root, _remote_root) = tests::init_repo();

	tests::run_git(&repo_root, &["checkout", "-b", "feature"]);

	let error = default_branch_sync::sync_repo_root_default_branch(&repo_root, "main", None)
		.expect_err("non-default repo root branch should be rejected");

	assert!(error.to_string().contains("is on branch `feature`"));
	assert!(error.to_string().contains("fast-forward local `main`"));
}
