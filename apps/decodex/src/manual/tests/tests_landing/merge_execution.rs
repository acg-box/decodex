use std::fs;

use tempfile::TempDir;

use crate::{
	github::RepositoryContext,
	manual::{self, LandExecutionMode, ManualAuthority, ManualLandContext, tests},
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
};

#[test]
fn execute_land_merge_uses_admin_merge() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");
	let (_path_guard, invocation_log_path) =
		tests::install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = ManualLandContext {
		cwd: checkout.clone(),
		current_branch: String::from("xy-225"),
		worktree_root: temp_dir.path().join(".worktrees"),
		project_worktree_root: temp_dir.path().join(".worktrees"),
		canonical_repo_root: checkout,
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
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: String::from("xy-225"),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};
	let merge_commit = manual::execute_land_merge(
		&context,
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/2","change":"ship hotfix","authority":"manual","impact":"compatible"}"#,
		LandExecutionMode::MergeAndCloseout,
	)
	.expect("manual land should admin-merge successfully");

	assert_eq!(merge_commit, "cafebabe");
	assert_eq!(
		fs::read_to_string(&invocation_log_path)
			.expect("fake gh invocation log should read")
			.lines()
			.collect::<Vec<_>>(),
		vec![
			"pr merge --admin --merge --match-head-commit deadbeefdeadbeefdeadbeefdeadbeefdeadbeef --subject {\"schema\":\"decodex/commit/2\",\"change\":\"ship hotfix\",\"authority\":\"manual\",\"impact\":\"compatible\"} --body  https://github.com/hack-ink/decodex/pull/64",
			"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
		]
	);
}

#[test]
fn execute_land_merge_tolerates_already_merged_merge_race() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");
	let (_path_guard, invocation_log_path) =
		tests::install_fake_admin_merge_gh_with_merge_exit_code(
			&temp_dir,
			"cafebabe",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			r#"{"schema":"decodex/commit/2","change":"ship hotfix","authority":"manual","impact":"compatible"}"#,
			1,
		);
	let context = ManualLandContext {
		cwd: checkout.clone(),
		current_branch: String::from("xy-225"),
		worktree_root: temp_dir.path().join(".worktrees"),
		project_worktree_root: temp_dir.path().join(".worktrees"),
		canonical_repo_root: checkout,
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
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: String::from("xy-225"),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};
	let merge_commit = manual::execute_land_merge(
		&context,
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/2","change":"ship hotfix","authority":"manual","impact":"compatible"}"#,
		LandExecutionMode::MergeAndCloseout,
	)
	.expect("manual land should accept an already-merged PR race");

	assert_eq!(merge_commit, "cafebabe");
	assert_eq!(
		fs::read_to_string(&invocation_log_path)
			.expect("fake gh invocation log should read")
			.lines()
			.collect::<Vec<_>>(),
		vec![
			"pr merge --admin --merge --match-head-commit deadbeefdeadbeefdeadbeefdeadbeefdeadbeef --subject {\"schema\":\"decodex/commit/2\",\"change\":\"ship hotfix\",\"authority\":\"manual\",\"impact\":\"compatible\"} --body  https://github.com/hack-ink/decodex/pull/64",
			"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
			"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
		]
	);
}
