use std::fs;

use tempfile::TempDir;

use crate::{
	github::RepositoryContext,
	manual::{self, ManualAuthority, ManualLandContext, tests},
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
	worktree::WorktreeManager,
};

#[test]
fn manual_land_issue_closeout_removes_managed_lane_worktree_and_branch() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");
	tests::git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

	let worktree_manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let worktree =
		worktree_manager.ensure_worktree("XY-225", false).expect("worktree should create");
	let (_path_guard, invocation_log_path) =
		tests::install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = ManualLandContext {
		cwd: worktree.path.clone(),
		current_branch: worktree.branch_name.clone(),
		worktree_root: worktree.path.clone(),
		project_worktree_root: worktree_root.clone(),
		canonical_repo_root: repo_root.clone(),
		authority: ManualAuthority::Issue(String::from("XY-225")),
		service_id: String::from("pubfi"),
		workflow: Some(tests::sample_workflow()),
		github_token_env_var: String::from("GITHUB_TOKEN"),
		github_token: String::from("test-token"),
		github_command_path: None,
		repository: RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		},
		prepared_closeout: None,
		review_lifecycle: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: worktree.branch_name.clone(),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};

	manual::cleanup_manual_land_lane_checkout(&context)
		.expect("manual land cleanup should remove the lane checkout");

	let gh_invocations =
		fs::read_to_string(invocation_log_path).expect("fake gh invocation log should read");

	assert!(
		gh_invocations
			.contains("api --method DELETE --silent repos/hack-ink/decodex/git/refs/heads/"),
		"manual land cleanup should delete the remote branch through gh api"
	);
	assert!(!worktree.path.exists(), "manual land cleanup should remove the worktree");
	assert!(
		manual::run_git_capture(&repo_root, &["branch", "--list", &worktree.branch_name])
			.expect("local branch list should run")
			.is_empty(),
		"manual land cleanup should delete the local lane branch"
	);
}

#[test]
fn manual_land_manual_authority_removes_managed_lane_worktree_and_branch() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");
	tests::git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

	let worktree_manager = WorktreeManager::new("decodex", &repo_root, &worktree_root);
	let worktree = worktree_manager
		.ensure_worktree("manual-land-cleanup", false)
		.expect("worktree should create");
	let (_path_guard, _invocation_log_path) =
		tests::install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = ManualLandContext {
		cwd: worktree.path.clone(),
		current_branch: worktree.branch_name.clone(),
		worktree_root: worktree.path.clone(),
		project_worktree_root: worktree_root.clone(),
		canonical_repo_root: repo_root.clone(),
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(tests::sample_workflow()),
		github_token_env_var: String::from("GITHUB_TOKEN"),
		github_token: String::from("test-token"),
		github_command_path: None,
		repository: RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		},
		prepared_closeout: None,
		review_lifecycle: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/65"),
		review_branch: worktree.branch_name.clone(),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};

	manual::cleanup_manual_land_lane_checkout(&context)
		.expect("manual authority cleanup should remove the lane checkout");

	assert!(!worktree.path.exists(), "manual authority cleanup should remove the worktree");
	assert!(
		manual::run_git_capture(&repo_root, &["branch", "--list", &worktree.branch_name])
			.expect("local branch list should run")
			.is_empty(),
		"manual authority cleanup should delete the local lane branch"
	);
}

#[test]
fn manual_land_issue_closeout_requires_managed_lane_checkout() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	let error =
		manual::ensure_manual_land_checkout_is_managed_lane(&repo_root, &worktree_root, "XY-225")
			.expect_err("issue closeout should require a managed lane checkout");

	assert!(error.to_string().contains("must run from a managed lane"));
	assert!(error.to_string().contains("XY-225"));
}
