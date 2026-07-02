use std::{fs, path::Path};

use tempfile::TempDir;

use crate::{
	github::RepositoryContext,
	manual::{self, LandExecutionMode, ManualAuthority, ManualLandContext, tests},
	state::{RUN_ACTIVITY_MARKER_FILE, RUN_CONTROL_CHANNEL_DIR},
	tracker::privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
};

#[test]
fn issue_identifier_helpers_recognize_lane_directory_names() {
	let inferred =
		manual::infer_issue_identifier_from_worktree_root(Path::new("/tmp/.worktrees/XY-225"))
			.expect("issue identifier should infer from worktree basename");

	assert_eq!(inferred, "XY-225");
	assert!(!manual::looks_like_issue_identifier("decodex"));
	assert!(!manual::looks_like_issue_identifier("feature-branch"));
	assert!(manual::looks_like_issue_identifier("xy-225"));
}

#[test]
fn landing_cleanliness_ignores_untracked_decodex_runtime_markers() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");

	fs::write(checkout.join(RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
		.expect("activity marker should write");

	let control_dir = checkout.join(RUN_CONTROL_CHANNEL_DIR);

	fs::create_dir_all(&control_dir).expect("run-control directory should create");
	fs::write(control_dir.join("run-1-1.channel"), "schema=decodex.run_control_channel/v1\n")
		.expect("run-control channel should write");
	manual::ensure_clean_worktree(&checkout)
		.expect("untracked Decodex runtime artifacts should not block landing");
}

#[test]
fn landing_cleanliness_rejects_blocking_worktree_statuses() {
	fn assert_blocks(checkout: &Path, case_name: &str) {
		let error = manual::ensure_clean_worktree(checkout).expect_err(case_name);

		assert!(
			error.to_string().contains("uncommitted changes"),
			"unexpected error for `{case_name}`: {error:?}"
		);
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");

		fs::write(checkout.join("scratch.txt"), "debug\n").expect("scratch file should write");

		assert_blocks(&checkout, "untracked non-runtime files should block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");
		let nested_dir = checkout.join("nested");

		fs::create_dir_all(&nested_dir).expect("nested directory should create");
		fs::write(nested_dir.join(RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
			.expect("nested activity marker should write");

		assert_blocks(&checkout, "nested runtime marker should still block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");
		let nested_control_dir = checkout.join("nested").join(RUN_CONTROL_CHANNEL_DIR);

		fs::create_dir_all(&nested_control_dir).expect("nested control directory should create");
		fs::write(nested_control_dir.join("run-1-1.channel"), "channel\n")
			.expect("nested control channel should write");

		assert_blocks(&checkout, "nested run-control directory should still block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = tests::init_git_checkout(&temp_dir, "repo");
		let marker_path = checkout.join(RUN_ACTIVITY_MARKER_FILE);

		fs::write(&marker_path, "idle\n").expect("activity marker should write");
		tests::git_add_and_commit(
			&checkout,
			RUN_ACTIVITY_MARKER_FILE,
			"track activity marker for test",
		);
		fs::write(&marker_path, "agent_run\n").expect("activity marker should update");

		assert_blocks(&checkout, "tracked runtime marker changes should block landing");
	}
}

#[test]
fn landing_state_validation_blocks_base_drift_except_after_merge() {
	let error = manual::validate_landing_state(
		&tests::sample_landing_state(),
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("non-default-base PR should be rejected");

	assert!(error.to_string().contains("targets base branch `release/1.x`"));
	assert!(error.to_string().contains("only lands into `main`"));

	let mut landing_state = tests::sample_landing_state();

	landing_state.state = String::from("MERGED");

	let mode = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"release/1.x",
		"XY-225",
		"deadbeef",
	)
	.expect("merged PR should resume closeout");

	assert_eq!(mode, LandExecutionMode::CloseoutOnly);
}

#[test]
fn landing_state_validation_explains_unknown_mergeability_after_retry() {
	let mut landing_state = tests::sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.mergeable = String::from("UNKNOWN");

	let error = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("unknown mergeability should not land");

	assert!(error.to_string().contains("mergeability is still unknown after retry"));
	assert!(error.to_string().contains("retry `decodex land`"));
}

#[test]
fn landing_state_validation_treats_pending_checks_as_wait_even_when_merge_blocked() {
	let mut landing_state = tests::sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.merge_state_status = String::from("BLOCKED");
	landing_state.status_check_rollup_state = Some(String::from("PENDING"));

	let error = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("pending checks should wait rather than report a generic blocked merge state");

	assert!(error.to_string().contains("still waiting on checks"));
	assert!(error.to_string().contains("statusCheckRollup=`PENDING`"));
}

#[test]
fn landing_state_validation_rejects_blocked_merge_state_after_green_gates() {
	let mut landing_state = tests::sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.merge_state_status = String::from("BLOCKED");

	let error = manual::validate_landing_state(
		&landing_state,
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("blocked merge state should not land without a policy change");

	assert!(error.to_string().contains("not ready to land"));
	assert!(error.to_string().contains("mergeStateStatus=`BLOCKED`"));
}

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
		review_handoff: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: String::from("xy-225"),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};
	let merge_commit = manual::execute_land_merge(
		&context,
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
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
			"pr merge --admin --merge --match-head-commit deadbeefdeadbeefdeadbeefdeadbeefdeadbeef --subject {\"schema\":\"decodex/commit/1\",\"summary\":\"ship hotfix\",\"authority\":\"manual\"} --body  https://github.com/hack-ink/decodex/pull/64",
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
			r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
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
		review_handoff: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: String::from("xy-225"),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};
	let merge_commit = manual::execute_land_merge(
		&context,
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
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
			"pr merge --admin --merge --match-head-commit deadbeefdeadbeefdeadbeefdeadbeefdeadbeef --subject {\"schema\":\"decodex/commit/1\",\"summary\":\"ship hotfix\",\"authority\":\"manual\"} --body  https://github.com/hack-ink/decodex/pull/64",
			"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
			"pr view https://github.com/hack-ink/decodex/pull/64 --json state,headRefOid,mergeCommit",
		]
	);
}

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
