use std::fs;

use tempfile::TempDir;

use crate::{
	github::RepositoryContext,
	manual::{self, ManualAuthority, ManualLandContext, tests},
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
};

#[test]
fn load_authoritative_landed_change_record_uses_merge_commit_subject() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");
	let (_path_guard, invocation_log_path) =
		tests::install_fake_admin_merge_gh_with_merge_exit_code(
			&temp_dir,
			"cafebabe",
			"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
			r#"{"schema":"decodex/commit/1","summary":"actual merge subject","authority":"manual"}"#,
			0,
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
		review_handoff: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: String::from("xy-225"),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};
	let landed_change_record =
		manual::load_authoritative_landed_change_record(&context, "cafebabe")
			.expect("merge commit subject should load");

	assert_eq!(
		landed_change_record,
		r#"{"schema":"decodex/commit/1","summary":"actual merge subject","authority":"manual"}"#
	);
	assert_eq!(
		fs::read_to_string(&invocation_log_path)
			.expect("fake gh invocation log should read")
			.lines()
			.collect::<Vec<_>>(),
		vec!["api repos/hack-ink/decodex/commits/cafebabe"]
	);
}
