use tempfile::TempDir;

use crate::manual::{self, ManualLandRequest, tests};

#[test]
fn manual_land_manual_authority_recovery_accepts_merged_pr_after_cleanup() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = tests::merge_manual_land_test_branch(&repo_root, &worktree_root);

	tests::remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);

	let context = tests::repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state =
		tests::merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);

	manual::ensure_already_merged_manual_land_recovery_ready(
		&context,
		&landing_state,
		&merged_pr.merge_commit,
	)
	.expect("already-merged manual land recovery should succeed after cleanup debt is gone");
}

#[test]
fn manual_land_manual_authority_recovery_entrypoint_accepts_merged_pr() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = tests::merge_manual_land_test_branch(&repo_root, &worktree_root);

	tests::remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);

	let _path_guard = tests::install_fake_landing_state_gh(
		&temp_dir,
		"MERGED",
		&merged_pr.branch_name,
		&merged_pr.head_oid,
		&merged_pr.merge_commit,
	);
	let context = tests::repo_root_manual_land_context(&repo_root, &worktree_root);
	let request = ManualLandRequest {
		summary: String::from("land manual PR"),
		authority: None,
		manual_authority: true,
		pr_url: Some(context.pr_url.clone()),
		breaking: false,
	};
	let outcome = manual::finalize_already_merged_manual_land_recovery(&context, &request)
		.expect("entrypoint should accept already-merged PR recovery")
		.expect("manual-authority recovery should run from repo-root main");

	assert_eq!(outcome.merge_commit, merged_pr.merge_commit);
}

#[test]
fn manual_land_manual_authority_recovery_rejects_unmerged_pr() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let context = tests::repo_root_manual_land_context(&repo_root, &worktree_root);
	let mut landing_state = tests::sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.head_ref_name = String::from("x/decodex-manual-land-cleanup");

	let error = manual::ensure_already_merged_manual_land_recovery_ready(
		&context,
		&landing_state,
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
	)
	.expect_err("unmerged PRs must not use default-branch recovery");

	assert!(error.to_string().contains("only accepts already-merged PRs"));
}

#[test]
fn manual_land_manual_authority_recovery_rejects_incomplete_lane_cleanup() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = tests::merge_manual_land_test_branch(&repo_root, &worktree_root);
	let context = tests::repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state =
		tests::merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
	let error = manual::ensure_already_merged_manual_land_recovery_ready(
		&context,
		&landing_state,
		&merged_pr.merge_commit,
	)
	.expect_err("recovery should reject when the landed lane branch remains");

	assert!(error.to_string().contains("landed lane cleanup to be complete"));
	assert!(error.to_string().contains(&merged_pr.branch_name));
}

#[test]
fn manual_land_manual_authority_recovery_rejects_detached_lane_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = tests::merge_manual_land_test_branch(&repo_root, &worktree_root);

	tests::git_success(&merged_pr.worktree_path, &["checkout", "--detach"]);
	tests::git_success(&repo_root, &["branch", "-D", &merged_pr.branch_name]);

	let context = tests::repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state =
		tests::merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
	let error = manual::ensure_already_merged_manual_land_recovery_ready(
		&context,
		&landing_state,
		&merged_pr.merge_commit,
	)
	.expect_err("recovery should reject a detached worktree at the landed PR head");

	assert!(error.to_string().contains("landed lane cleanup to be complete"));
	assert!(error.to_string().contains(&merged_pr.head_oid));
}

#[test]
fn manual_land_manual_authority_recovery_rejects_remaining_cleanup_debt() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = tests::merge_manual_land_test_branch(&repo_root, &worktree_root);

	tests::remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);
	tests::create_dirty_merged_worktree_debt(&repo_root, &worktree_root);

	let context = tests::repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state =
		tests::merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
	let error = manual::ensure_already_merged_manual_land_recovery_ready(
		&context,
		&landing_state,
		&merged_pr.merge_commit,
	)
	.expect_err("recovery should reject remaining merged worktree cleanup debt");

	assert!(error.to_string().contains("post-land worktree cleanup debt remains"));
	assert!(error.to_string().contains("XY-999"));
}
