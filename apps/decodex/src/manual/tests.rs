pub(super) mod support;

mod tests_authority;
mod tests_cleanup;
mod tests_landing;
mod tests_records;
mod tests_recovery;

use std::{
	env, fs,
	os::unix::fs::PermissionsExt,
	path::{Path, PathBuf},
};

use tempfile::TempDir;

#[rustfmt::skip]
use crate::test_support;
use self::support::TestTracker;
#[rustfmt::skip]
use crate::{config::ServiceConfig, manual::{
		self, LandExecutionMode, ManualAuthority, ManualLandLedgerContext, ManualLandRequest,
		RepositoryContext, ReviewHandoffMarker, StateStore, authority, closeout, landing, recovery,
	}, pull_request::PullRequestLandingState, runtime, state::{RUN_ACTIVITY_MARKER_FILE, RUN_CONTROL_CHANNEL_DIR}, test_support::{TestEnvVarGuard}, tracker::{
		TrackerIssue, TrackerLabel, TrackerState, TrackerTeam,
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier, records,
	}, workflow::WorkflowDocument, worktree::WorktreeManager};

struct MergedManualLandBranch {
	branch_name: String,
	head_oid: String,
	merge_commit: String,
	worktree_path: PathBuf,
}

fn init_git_checkout(temp_dir: &TempDir, directory_name: &str) -> PathBuf {
	let checkout = temp_dir.path().join(directory_name);

	assert!(
		test_support::hermetic_git_command()
			.args(["init", "-b", "main"])
			.current_dir(temp_dir.path())
			.arg(&checkout)
			.status()
			.expect("git init should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.name", "Decodex Tests"])
			.current_dir(&checkout)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.email", "decodex-tests@example.com"])
			.current_dir(&checkout)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "commit.gpgsign", "false"])
			.current_dir(&checkout)
			.status()
			.expect("git config should run")
			.success()
	);

	checkout
}

fn git_success(cwd: &Path, args: &[&str]) {
	assert!(
		test_support::hermetic_git_command()
			.args(args)
			.current_dir(cwd)
			.status()
			.expect("git command should run")
			.success(),
		"git {:?} should succeed",
		args
	);
}

fn git_add_and_commit(cwd: &Path, pathspec: &str, message: &str) {
	assert!(
		test_support::hermetic_git_command()
			.args(["add", pathspec])
			.current_dir(cwd)
			.status()
			.expect("git add should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["commit", "-m", message])
			.current_dir(cwd)
			.status()
			.expect("git commit should run")
			.success()
	);
}

fn init_git_checkout_with_origin(temp_dir: &TempDir) -> PathBuf {
	let remote_root = temp_dir.path().join("origin.git");
	let checkout = temp_dir.path().join("repo");

	assert!(
		test_support::hermetic_git_command()
			.args(["init", "--bare", "--initial-branch", "main"])
			.arg(&remote_root)
			.status()
			.expect("bare origin should init")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["clone"])
			.arg(&remote_root)
			.arg(&checkout)
			.status()
			.expect("repo should clone")
			.success()
	);

	git_success(&checkout, &["config", "user.name", "Decodex Tests"]);
	git_success(&checkout, &["config", "user.email", "decodex-tests@example.com"]);
	git_success(&checkout, &["config", "commit.gpgsign", "false"]);

	fs::write(checkout.join("README.md"), "bootstrap\n").expect("readme should write");

	git_add_and_commit(&checkout, "README.md", "bootstrap repo");
	git_success(&checkout, &["push", "origin", "main"]);

	checkout
}

fn repo_root_manual_land_context(
	repo_root: &Path,
	worktree_root: &Path,
) -> crate::manual::ManualLandContext {
	crate::manual::ManualLandContext {
		cwd: repo_root.to_path_buf(),
		current_branch: String::from("main"),
		worktree_root: repo_root.to_path_buf(),
		project_worktree_root: worktree_root.to_path_buf(),
		canonical_repo_root: repo_root.to_path_buf(),
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(sample_workflow()),
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
		review_branch: String::from("main"),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	}
}

fn merge_manual_land_test_branch(repo_root: &Path, worktree_root: &Path) -> MergedManualLandBranch {
	let worktree_manager = WorktreeManager::new("decodex", repo_root, worktree_root);
	let worktree = worktree_manager
		.ensure_worktree("manual-land-cleanup", false)
		.expect("manual land worktree should create");

	fs::write(worktree.path.join("feature.txt"), "manual land\n")
		.expect("feature file should write");

	git_add_and_commit(&worktree.path, "feature.txt", "manual land feature");

	let head_oid = manual::run_git_capture(&worktree.path, &["rev-parse", "HEAD"])
		.expect("PR head should read");

	git_success(repo_root, &["merge", "--no-ff", &worktree.branch_name, "-m", "land feature"]);

	let merge_commit =
		manual::run_git_capture(repo_root, &["rev-parse", "HEAD"]).expect("merge head");

	git_success(repo_root, &["push", "origin", "main"]);

	MergedManualLandBranch {
		branch_name: worktree.branch_name,
		head_oid,
		merge_commit,
		worktree_path: worktree.path,
	}
}

fn remove_test_lane_checkout(repo_root: &Path, worktree_path: &Path, branch_name: &str) {
	git_success(worktree_path, &["checkout", "--detach"]);
	git_success(repo_root, &["branch", "-D", branch_name]);
	git_success(
		repo_root,
		&[
			"worktree",
			"remove",
			"--force",
			worktree_path.to_str().expect("worktree path should be UTF-8"),
		],
	);
}

fn create_dirty_merged_worktree_debt(repo_root: &Path, worktree_root: &Path) {
	let worktree_manager = WorktreeManager::new("decodex", repo_root, worktree_root);
	let worktree =
		worktree_manager.ensure_worktree("XY-999", false).expect("debt worktree should create");

	fs::write(worktree.path.join("debt.txt"), "debt\n").expect("debt file should write");

	git_add_and_commit(&worktree.path, "debt.txt", "debt feature");
	git_success(repo_root, &["merge", "--no-ff", &worktree.branch_name, "-m", "land debt"]);
	git_success(repo_root, &["push", "origin", "main"]);

	fs::write(worktree.path.join("debt.txt"), "dirty debt\n")
		.expect("debt worktree should become dirty");
}

fn merged_manual_land_state(branch_name: &str, head_oid: &str) -> PullRequestLandingState {
	let mut landing_state = sample_landing_state();

	landing_state.state = String::from("MERGED");
	landing_state.base_ref_name = String::from("main");
	landing_state.head_ref_name = branch_name.to_owned();
	landing_state.head_ref_oid = head_oid.to_owned();

	landing_state
}

fn install_fake_landing_state_gh(
	temp_dir: &TempDir,
	state: &str,
	branch_name: &str,
	head_oid: &str,
	merge_commit: &str,
) -> TestEnvVarGuard {
	let fake_gh_dir = temp_dir.path().join("fake-recovery-bin");
	let fake_gh_path = fake_gh_dir.join("gh");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"api\" ] && [ \"$2\" = \"graphql\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			serde_json::json!({
				"data": {
					"repository": {
						"pullRequest": {
							"url": "https://github.com/hack-ink/decodex/pull/64",
							"state": state,
							"isDraft": false,
							"reviewDecision": "APPROVED",
							"baseRefName": "main",
							"mergeable": "MERGEABLE",
							"mergeStateStatus": "CLEAN",
							"headRefName": branch_name,
							"headRefOid": head_oid,
							"reviewRequests": { "totalCount": 0 },
							"reviewThreads": {
								"nodes": [],
								"pageInfo": { "hasNextPage": false, "endCursor": null },
							},
							"commits": {
								"nodes": [
									{
										"commit": {
											"statusCheckRollup": { "state": "SUCCESS" },
										},
									},
								],
							},
						},
					},
				},
			}),
			serde_json::json!({
				"state": state,
				"headRefOid": head_oid,
				"mergeCommit": { "oid": merge_commit },
			}),
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	{
		PermissionsExt::set_mode(&mut permissions, 0o755);
	}

	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display()))
}

fn install_fake_repo_view_gh(temp_dir: &TempDir) -> PathBuf {
	let fake_gh_dir = temp_dir.path().join("fake-repo-view-bin");
	let fake_gh_path = fake_gh_dir.join("gh");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
if [ \"$1\" = \"repo\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"auth\" ] && [ \"$2\" = \"token\" ]; then\n\
  printf '%s\\n' 'ghp_fake_auth_token'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			serde_json::json!({
				"name": "decodex",
				"owner": { "login": "hack-ink" },
				"defaultBranchRef": { "name": "main" },
				"mergeCommitAllowed": true,
			}),
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	{
		PermissionsExt::set_mode(&mut permissions, 0o755);
	}

	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	fake_gh_dir
}

fn sample_workflow() -> WorkflowDocument {
	WorkflowDocument::parse_markdown(
		r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 8
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Test workflow.
"#,
	)
	.expect("sample workflow should parse")
}

fn install_fake_admin_merge_gh(
	temp_dir: &TempDir,
	merged_head_oid: &str,
) -> (TestEnvVarGuard, PathBuf) {
	install_fake_admin_merge_gh_with_merge_exit_code(
		temp_dir,
		merged_head_oid,
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
		0,
	)
}

fn install_fake_admin_merge_gh_with_merge_exit_code(
	temp_dir: &TempDir,
	merged_head_oid: &str,
	pr_head_oid: &str,
	merge_subject: &str,
	merge_exit_code: i32,
) -> (TestEnvVarGuard, PathBuf) {
	let fake_gh_dir = temp_dir.path().join("fake-bin");
	let fake_gh_path = fake_gh_dir.join("gh");
	let invocation_log_path = temp_dir.path().join("gh-invocation.log");

	fs::create_dir_all(&fake_gh_dir).expect("fake gh directory should exist");
	fs::write(
		&fake_gh_path,
		format!(
			"#!/bin/sh\n\
printf '%s\\n' \"$*\" >> '{}'\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"merge\" ]; then\n\
  exit {}\n\
fi\n\
if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
if [ \"$1\" = \"api\" ]; then\n\
  printf '%s' '{}'\n\
  exit 0\n\
fi\n\
echo \"unexpected gh invocation: $*\" >&2\n\
exit 1\n",
			invocation_log_path.display(),
			merge_exit_code,
			serde_json::json!({
				"state": "MERGED",
				"headRefOid": pr_head_oid,
				"mergeCommit": { "oid": merged_head_oid },
			}),
			serde_json::json!({
				"commit": { "message": format!("{merge_subject}\n\n") },
			}),
		),
	)
	.expect("fake gh script should write");

	let mut permissions =
		fs::metadata(&fake_gh_path).expect("fake gh metadata should read").permissions();

	#[cfg(unix)]
	{
		PermissionsExt::set_mode(&mut permissions, 0o755);
	}

	fs::set_permissions(&fake_gh_path, permissions)
		.expect("fake gh script should become executable");

	let path_env = env::var("PATH").unwrap_or_default();

	(
		TestEnvVarGuard::set("PATH", &format!("{}:{path_env}", fake_gh_dir.display())),
		invocation_log_path,
	)
}

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
	let checkout = init_git_checkout(&temp_dir, "repo");

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
		let checkout = init_git_checkout(&temp_dir, "repo");

		fs::write(checkout.join("scratch.txt"), "debug\n").expect("scratch file should write");

		assert_blocks(&checkout, "untracked non-runtime files should block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");
		let nested_dir = checkout.join("nested");

		fs::create_dir_all(&nested_dir).expect("nested directory should create");
		fs::write(nested_dir.join(RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
			.expect("nested activity marker should write");

		assert_blocks(&checkout, "nested runtime marker should still block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");
		let nested_control_dir = checkout.join("nested").join(RUN_CONTROL_CHANNEL_DIR);

		fs::create_dir_all(&nested_control_dir).expect("nested control directory should create");
		fs::write(nested_control_dir.join("run-1-1.channel"), "channel\n")
			.expect("nested control channel should write");

		assert_blocks(&checkout, "nested run-control directory should still block landing");
	}
	{
		let temp_dir = TempDir::new().expect("temp dir should create");
		let checkout = init_git_checkout(&temp_dir, "repo");
		let marker_path = checkout.join(RUN_ACTIVITY_MARKER_FILE);

		fs::write(&marker_path, "idle\n").expect("activity marker should write");

		git_add_and_commit(&checkout, RUN_ACTIVITY_MARKER_FILE, "track activity marker for test");

		fs::write(&marker_path, "agent_run\n").expect("activity marker should update");

		assert_blocks(&checkout, "tracked runtime marker changes should block landing");
	}
}

#[test]
fn landing_state_validation_blocks_base_drift_except_after_merge() {
	let error = landing::validate_landing_state(
		&sample_landing_state(),
		"https://github.com/hack-ink/decodex/pull/64",
		"main",
		"XY-225",
		"deadbeef",
	)
	.expect_err("non-default-base PR should be rejected");

	assert!(error.to_string().contains("targets base branch `release/1.x`"));
	assert!(error.to_string().contains("only lands into `main`"));

	let mut landing_state = sample_landing_state();

	landing_state.state = String::from("MERGED");

	let mode = landing::validate_landing_state(
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
	let mut landing_state = sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.mergeable = String::from("UNKNOWN");

	let error = landing::validate_landing_state(
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
	let mut landing_state = sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.merge_state_status = String::from("BLOCKED");
	landing_state.status_check_rollup_state = Some(String::from("PENDING"));

	let error = landing::validate_landing_state(
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
	let mut landing_state = sample_landing_state();

	landing_state.base_ref_name = String::from("main");
	landing_state.merge_state_status = String::from("BLOCKED");

	let error = landing::validate_landing_state(
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
	let checkout = init_git_checkout(&temp_dir, "repo");
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = manual::ManualLandContext {
		cwd: checkout.clone(),
		current_branch: String::from("xy-225"),
		worktree_root: temp_dir.path().join(".worktrees"),
		project_worktree_root: temp_dir.path().join(".worktrees"),
		canonical_repo_root: checkout,
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(sample_workflow()),
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
	let merge_commit = landing::execute_land_merge(
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
	let checkout = init_git_checkout(&temp_dir, "repo");
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_with_merge_exit_code(
		&temp_dir,
		"cafebabe",
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/1","summary":"ship hotfix","authority":"manual"}"#,
		1,
	);
	let context = manual::ManualLandContext {
		cwd: checkout.clone(),
		current_branch: String::from("xy-225"),
		worktree_root: temp_dir.path().join(".worktrees"),
		project_worktree_root: temp_dir.path().join(".worktrees"),
		canonical_repo_root: checkout,
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(sample_workflow()),
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
	let merge_commit = landing::execute_land_merge(
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
	let checkout = init_git_checkout(&temp_dir, "repo");
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_with_merge_exit_code(
		&temp_dir,
		"cafebabe",
		"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
		r#"{"schema":"decodex/commit/1","summary":"actual merge subject","authority":"manual"}"#,
		0,
	);
	let context = manual::ManualLandContext {
		cwd: checkout.clone(),
		current_branch: String::from("xy-225"),
		worktree_root: temp_dir.path().join(".worktrees"),
		project_worktree_root: temp_dir.path().join(".worktrees"),
		canonical_repo_root: checkout,
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(sample_workflow()),
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
		landing::load_authoritative_landed_change_record(&context, "cafebabe")
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

#[test]
fn land_authority_validates_issue_override_against_lane() {
	let error = manual::resolve_land_authority(
		Some(Path::new("/tmp/project.toml")),
		Some("XY-999"),
		false,
		Path::new("/tmp/.worktrees/XY-225"),
	)
	.expect_err("mismatched explicit authority should be rejected");

	assert!(error.to_string().contains("does not match the current lane issue `XY-225`"));

	let authority = manual::resolve_land_authority(
		Some(Path::new("/tmp/project.toml")),
		Some("XY-225"),
		false,
		Path::new("/tmp/.worktrees/xy-225"),
	)
	.expect("same issue with different casing should be accepted");

	assert_eq!(authority, ManualAuthority::Issue(String::from("xy-225")));
}

#[test]
fn resolve_authority_accepts_manual_authority() {
	let authority = authority::resolve_authority(
		Some(Path::new("/tmp/project.toml")),
		None,
		true,
		Path::new("/tmp/worktree"),
	)
	.expect("manual authority should resolve");

	assert_eq!(authority, ManualAuthority::Manual);
}

#[test]
fn manual_land_manual_authority_without_config_prepares_unregistered_context() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = init_git_checkout(&temp_dir, "repo");
	let fake_gh_dir = install_fake_repo_view_gh(&temp_dir);
	let path_env = env::var("PATH").unwrap_or_default();
	let _env_guard = TestEnvVarGuard::set_many([
		("GH_TOKEN", String::from("ghp_test")),
		("PATH", format!("{}:{path_env}", fake_gh_dir.display())),
	]);
	let request = ManualLandRequest {
		summary: String::from("ship hotfix"),
		authority: None,
		manual_authority: true,
		pr_url: Some(String::from("https://github.com/hack-ink/decodex/pull/64")),
		related: Vec::new(),
		breaking: false,
	};
	let context = manual::prepare_unregistered_manual_land_context(
		repo_root.clone(),
		repo_root.clone(),
		String::from("main"),
		&request,
	)
	.expect("manual land should prepare without project registry");

	assert_eq!(context.authority, ManualAuthority::Manual);
	assert_eq!(context.service_id, "decodex");
	assert_eq!(
		context.canonical_repo_root,
		fs::canonicalize(&repo_root).expect("repo root should canonicalize")
	);
	assert_eq!(context.project_worktree_root, context.canonical_repo_root.join(".worktrees"));
	assert!(context.workflow.is_none());
	assert!(context.prepared_closeout.is_none());
	assert!(context.review_handoff.is_none());
	assert_eq!(context.github_token_env_var, "GH_TOKEN");
	assert_eq!(context.github_token, "ghp_test");
}

#[test]
fn manual_land_manual_authority_with_config_does_not_refresh_project_registry() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let fake_gh_dir = install_fake_repo_view_gh(&temp_dir);
	let path_env = env::var("PATH").unwrap_or_default();
	let _home_guard = TestEnvVarGuard::set_many([
		("HOME", temp_dir.path().to_str().expect("temp dir path should be utf-8").to_owned()),
		("GH_TOKEN", String::from("ghp_test")),
		("PATH", format!("{}:{path_env}", fake_gh_dir.display())),
	]);
	let repo_root = init_git_checkout(&temp_dir, "repo");
	let config_dir = temp_dir.path().join(".codex/decodex/projects/decodex");
	let config_path = config_dir.join("project.toml");
	let request = ManualLandRequest {
		summary: String::from("ship hotfix"),
		authority: None,
		manual_authority: true,
		pr_url: Some(String::from("https://github.com/hack-ink/decodex/pull/64")),
		related: Vec::new(),
		breaking: false,
	};

	fs::create_dir_all(&config_dir).expect("config dir should exist");
	fs::write(
		&config_path,
		format!(
			r#"
			service_id = "decodex"

			[tracker]
			api_key_env_var = "GH_TOKEN"

			[github]
			token_env_var = "GH_TOKEN"

			[paths]
			repo_root = "{}"
			"#,
			repo_root.display()
		),
	)
	.expect("project config should write");
	fs::write(
		config_dir.join("WORKFLOW.md"),
		sample_workflow().to_markdown().expect("sample workflow should render"),
	)
	.expect("workflow should write");

	let context = manual::prepare_configured_manual_land_context(
		repo_root,
		temp_dir.path().join(".worktrees/XY-225"),
		String::from("xy-225"),
		&config_path,
		&request,
	)
	.expect("manual land should prepare with config");
	let state_store = runtime::open_runtime_store().expect("runtime store should open");

	assert!(context.workflow.is_some());
	assert!(context.prepared_closeout.is_none());
	assert!(
		state_store.list_projects().expect("project registry should list").is_empty(),
		"manual-authority land should not refresh project registry unless issue closeout needs runtime state"
	);
}

#[test]
fn manual_commit_blocker_rejects_active_claimed_managed_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = temp_dir.path().join("XY-225");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"decodex",
			"issue-1",
			"y/decodex-xy-225",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should persist");
	state_store
		.upsert_lease("decodex", "issue-1", "run-1", "In Progress")
		.expect("active lease should persist");

	let blocker = manual::manual_commit_active_lane_blocker(
		&state_store,
		"decodex",
		&worktree_path,
		Some("y/decodex-xy-225"),
	)
	.expect("manual commit blocker should evaluate")
	.expect("active managed worktree should block");

	assert_eq!(blocker.issue_id, "issue-1");
	assert_eq!(blocker.branch_name, "y/decodex-xy-225");
	assert_eq!(blocker.worktree_path, worktree_path);
}

#[test]
fn manual_commit_blocker_allows_unclaimed_or_unmapped_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_path = temp_dir.path().join("XY-225");
	let other_path = temp_dir.path().join("XY-226");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	fs::create_dir_all(&other_path).expect("other worktree path should exist");

	state_store
		.upsert_worktree(
			"decodex",
			"issue-1",
			"y/decodex-xy-225",
			&worktree_path.display().to_string(),
		)
		.expect("worktree mapping should persist");

	assert!(
		manual::manual_commit_active_lane_blocker(
			&state_store,
			"decodex",
			&worktree_path,
			Some("y/decodex-xy-225"),
		)
		.expect("unclaimed worktree should evaluate")
		.is_none()
	);

	state_store
		.upsert_lease("decodex", "issue-1", "run-1", "In Progress")
		.expect("active lease should persist");

	assert!(
		manual::manual_commit_active_lane_blocker(
			&state_store,
			"decodex",
			&other_path,
			Some("y/decodex-xy-226"),
		)
		.expect("unmapped worktree should evaluate")
		.is_none()
	);
}

#[test]
fn resolve_pr_url_requires_explicit_pr_for_manual_authority() {
	let error = manual::resolve_pr_url(None, None, true)
		.expect_err("manual authority land should require explicit pr");

	assert!(error.to_string().contains("--manual-authority"));
}

#[test]
fn prepare_closeout_matches_authority_case_insensitively() {
	assert_eq!("xy-225".to_ascii_uppercase(), "XY-225");
}

#[test]
fn manual_closeout_scope_requires_service_ownership() {
	let issue = sample_issue("issue-1", "XY-225", false, &[]);
	let error = manual::ensure_manual_closeout_issue_scope(&TestTracker::new(), &issue, "pubfi")
		.expect_err("service ownership should be required");

	assert!(error.to_string().contains("decodex:active:pubfi"));

	let issue = sample_issue("issue-1", "XY-225", false, &[]);
	let tracker = TestTracker::new().with_label_issues("decodex:active:pubfi", vec![issue.clone()]);

	manual::ensure_manual_closeout_issue_scope(&tracker, &issue, "pubfi")
		.expect("server-confirmed service ownership should pass");
}

#[test]
fn manual_closeout_clear_removes_present_transient_decodex_labels() {
	for (case_name, labels, expected_label_ids) in [
		(
			"all transient labels present",
			&["decodex:active:pubfi", "decodex:queued:pubfi", "decodex:needs-attention"][..],
			&["team-label-0", "team-label-1", "team-label-2"][..],
		),
		("optional transient labels absent", &["decodex:active:pubfi"][..], &["team-label-0"][..]),
	] {
		let issue = sample_issue("issue-1", "XY-225", true, labels);
		let tracker = TestTracker::new();

		manual::clear_manual_closeout_issue_scope(
			&tracker,
			&issue,
			"pubfi",
			"decodex:needs-attention",
		)
		.expect(case_name);

		let expected_removals = expected_label_ids
			.iter()
			.map(|label_id| vec![(*label_id).to_owned()])
			.collect::<Vec<_>>();

		assert_eq!(tracker.label_removals.borrow().as_slice(), expected_removals.as_slice());
	}
}

#[test]
fn manual_closeout_clear_classifies_label_removal_errors() {
	for (case_name, labels, message, expected_label_ids, expected_error) in [
		(
			"missing label removal is idempotent",
			&["decodex:active:pubfi", "decodex:queued:pubfi", "decodex:needs-attention"][..],
			"Linear GraphQL request failed: Label not on issue",
			&["team-label-0", "team-label-1", "team-label-2"][..],
			None,
		),
		(
			"other label removal errors are preserved",
			&["decodex:active:pubfi"][..],
			"Linear GraphQL request failed: Timeout",
			&["team-label-0"][..],
			Some("Timeout"),
		),
	] {
		let issue = sample_issue("issue-1", "XY-225", true, labels);
		let tracker = TestTracker::new().with_label_removal_error(message);
		let result = manual::clear_manual_closeout_issue_scope(
			&tracker,
			&issue,
			"pubfi",
			"decodex:needs-attention",
		);

		if let Some(expected_error) = expected_error {
			let error = result.expect_err(case_name);

			assert!(error.to_string().contains(expected_error));
		} else {
			result.expect(case_name);
		}

		let expected_removals = expected_label_ids
			.iter()
			.map(|label_id| vec![(*label_id).to_owned()])
			.collect::<Vec<_>>();

		assert_eq!(tracker.label_removals.borrow().as_slice(), expected_removals.as_slice());
	}
}

#[test]
fn manual_closeout_runtime_clear_removes_lane_state() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
	let other_issue = sample_issue("issue-2", "XY-226", true, &["decodex:active:pubfi"]);
	let handoff = ReviewHandoffMarker::new(
		"run-1-failed",
		1,
		"y/decodex-xy-225",
		"https://github.com/hack-ink/decodex/pull/67",
		"main",
		"y/decodex-xy-225",
		"deadbeef",
	);

	state_store
		.upsert_lease("decodex", &issue.id, "run-1", "In Progress")
		.expect("issue lease should persist");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("issue running attempt should persist");
	state_store
		.record_run_attempt("run-1-starting", &issue.id, 2, "starting")
		.expect("issue starting attempt should persist");
	state_store
		.record_run_attempt("run-1-failed", &issue.id, 3, "failed")
		.expect("issue terminal attempt should persist");
	state_store
		.upsert_worktree("decodex", &issue.id, "y/decodex-xy-225", "/tmp/worktrees/xy-225")
		.expect("issue worktree should persist");
	state_store
		.upsert_review_handoff_marker("decodex", &issue.id, &handoff)
		.expect("issue handoff should persist");
	state_store
		.upsert_lease("decodex", &other_issue.id, "run-2", "In Progress")
		.expect("other issue lease should persist");
	state_store
		.record_run_attempt("run-2", &other_issue.id, 1, "running")
		.expect("other issue running attempt should persist");

	manual::clear_manual_closeout_runtime_state(&state_store, &issue.id, handoff.run_id())
		.expect("manual closeout runtime state should clear");

	assert!(
		state_store
			.list_leases("decodex")
			.expect("leases should list")
			.iter()
			.all(|lease| lease.issue_id() != issue.id)
	);
	assert!(
		state_store
			.list_leases("decodex")
			.expect("leases should list")
			.iter()
			.any(|lease| lease.issue_id() == other_issue.id)
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none()
	);
	assert!(
		state_store
			.review_handoff_marker("decodex", &issue.id, "y/decodex-xy-225")
			.expect("handoff lookup should succeed")
			.is_none()
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1-starting")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1-failed")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-2")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"running"
	);
}

#[test]
fn manual_land_issue_closeout_removes_managed_lane_worktree_and_branch() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

	git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

	let worktree_manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let worktree =
		worktree_manager.ensure_worktree("XY-225", false).expect("worktree should create");
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = manual::ManualLandContext {
		cwd: worktree.path.clone(),
		current_branch: worktree.branch_name.clone(),
		worktree_root: worktree.path.clone(),
		project_worktree_root: worktree_root.clone(),
		canonical_repo_root: repo_root.clone(),
		authority: ManualAuthority::Issue(String::from("XY-225")),
		service_id: String::from("pubfi"),
		workflow: Some(sample_workflow()),
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
	let repo_root = init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

	git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

	let worktree_manager = WorktreeManager::new("decodex", &repo_root, &worktree_root);
	let worktree = worktree_manager
		.ensure_worktree("manual-land-cleanup", false)
		.expect("worktree should create");
	let (_path_guard, _invocation_log_path) = install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = manual::ManualLandContext {
		cwd: worktree.path.clone(),
		current_branch: worktree.branch_name.clone(),
		worktree_root: worktree.path.clone(),
		project_worktree_root: worktree_root.clone(),
		canonical_repo_root: repo_root.clone(),
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(sample_workflow()),
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
fn manual_land_manual_authority_recovery_accepts_merged_pr_after_cleanup() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

	remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);

	let context = repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);

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
	let repo_root = init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

	remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);

	let _path_guard = install_fake_landing_state_gh(
		&temp_dir,
		"MERGED",
		&merged_pr.branch_name,
		&merged_pr.head_oid,
		&merged_pr.merge_commit,
	);
	let context = repo_root_manual_land_context(&repo_root, &worktree_root);
	let request = ManualLandRequest {
		summary: String::from("land manual PR"),
		authority: None,
		manual_authority: true,
		pr_url: Some(context.pr_url.clone()),
		related: Vec::new(),
		breaking: false,
	};
	let outcome = recovery::finalize_already_merged_manual_land_recovery(&context, &request)
		.expect("entrypoint should accept already-merged PR recovery")
		.expect("manual-authority recovery should run from repo-root main");

	assert_eq!(outcome.merge_commit, merged_pr.merge_commit);
}

#[test]
fn manual_land_manual_authority_recovery_rejects_unmerged_pr() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let context = repo_root_manual_land_context(&repo_root, &worktree_root);
	let mut landing_state = sample_landing_state();

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
	let repo_root = init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);
	let context = repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
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
	let repo_root = init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

	git_success(&merged_pr.worktree_path, &["checkout", "--detach"]);
	git_success(&repo_root, &["branch", "-D", &merged_pr.branch_name]);

	let context = repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
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
	let repo_root = init_git_checkout_with_origin(&temp_dir);
	let worktree_root = repo_root.join(".worktrees");
	let merged_pr = merge_manual_land_test_branch(&repo_root, &worktree_root);

	remove_test_lane_checkout(&repo_root, &merged_pr.worktree_path, &merged_pr.branch_name);
	create_dirty_merged_worktree_debt(&repo_root, &worktree_root);

	let context = repo_root_manual_land_context(&repo_root, &worktree_root);
	let landing_state = merged_manual_land_state(&merged_pr.branch_name, &merged_pr.head_oid);
	let error = manual::ensure_already_merged_manual_land_recovery_ready(
		&context,
		&landing_state,
		&merged_pr.merge_commit,
	)
	.expect_err("recovery should reject remaining merged worktree cleanup debt");

	assert!(error.to_string().contains("post-land worktree cleanup debt remains"));
	assert!(error.to_string().contains("XY-999"));
}

#[test]
fn manual_land_issue_closeout_requires_managed_lane_checkout() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	let error =
		closeout::ensure_manual_land_checkout_is_managed_lane(&repo_root, &worktree_root, "XY-225")
			.expect_err("issue closeout should require a managed lane checkout");

	assert!(error.to_string().contains("must run from a managed lane"));
	assert!(error.to_string().contains("XY-225"));
}

#[test]
fn manual_land_issue_closeout_writes_success_ledger_after_existing_marker() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = init_git_checkout(&temp_dir, "repo");
	let tracker = TestTracker::new();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = sample_issue("issue-1", "PUB-1161", true, &[]);

	issue
		.team
		.states
		.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });

	let handoff = ReviewHandoffMarker::new(
		String::from("pub-1161-attempt-1"),
		1,
		String::from("xy/pub-1161"),
		String::from("https://github.com/helixbox/pubfi-mono-v2/pull/95"),
		String::from("main"),
		String::from("xy/pub-1161"),
		String::from("3cf2d24033527a774340c7d70c5ce437c90afe55"),
	);

	state_store
		.record_run_attempt(handoff.run_id(), &issue.id, handoff.attempt_number(), "failed")
		.expect("failed handoff attempt should record");

	let merge_commit = "81e90b530148a0be69afa5bd33ce6ab84d485a3a";
	let landed_change_record =
		r#"{"schema":"decodex/commit/1","summary":"Land PUB-1161","authority":"PUB-1161"}"#;

	manual::write_manual_land_closeout_marker(
		&checkout,
		"https://github.com/helixbox/pubfi-mono-v2/pull/95",
		merge_commit,
		"xy/pub-1161",
		landed_change_record,
	)
	.expect("existing closeout marker should write");

	let ledger = ManualLandLedgerContext {
		service_id: "pubfi",
		issue: &issue,
		state_store: &state_store,
		handoff: &handoff,
		pr_url: "https://github.com/helixbox/pubfi-mono-v2/pull/95",
		merge_commit,
		branch_name: "xy/pub-1161",
		worktree_path: ".worktrees/PUB-1161",
		completed_state: "Done",
		default_branch: "main",
		privacy_classifier: &ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};

	manual::apply_closeout(&checkout, &tracker, "Done", &ledger, landed_change_record)
		.expect("manual closeout should write landed and closeout events");
	manual::write_manual_land_cleanup_complete_event(&tracker, &ledger)
		.expect("manual cleanup should write cleanup_complete event");

	let comments = tracker.comments.borrow();
	let records = comments
		.iter()
		.filter_map(|comment| records::parse_linear_execution_event_record(comment))
		.collect::<Vec<_>>();
	let event_types = records.iter().map(|record| record.event_type.as_str()).collect::<Vec<_>>();

	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[vec![String::from("issue-1"), String::from("state-done"),]]
	);
	assert_eq!(event_types, vec!["landed", "closeout", "cleanup_complete"]);
	assert!(
		comments.iter().all(|comment| !comment.starts_with("decodex land completed")),
		"matching legacy closeout marker should not replay the ordinary closeout comment"
	);
	assert!(comments.iter().all(|comment| {
		comment.contains("- run_sequence_attempt: `1` (not retry-budget count)")
			&& !comment.contains("- attempt:")
	}));
	assert!(records.iter().all(|record| record.run_id == "pub-1161-attempt-1"));
	assert!(records.iter().all(|record| record.attempt_number == 1));
	assert_eq!(records[0].pr_head_sha.as_deref(), Some(handoff.pr_head_oid()));
	assert_eq!(records[0].commit_sha.as_deref(), Some(merge_commit));
	assert_eq!(records[1].target_state.as_deref(), Some("Done"));
	assert_eq!(records[2].cleanup_status.as_deref(), Some("completed"));

	let cached_records = state_store
		.list_linear_execution_events("pubfi", "issue-1")
		.expect("local ledger cache should read");
	let cached_event_types =
		cached_records.iter().map(|record| record.event_type.as_str()).collect::<Vec<_>>();

	assert_eq!(cached_event_types, vec!["landed", "closeout", "cleanup_complete"]);
	assert_eq!(
		state_store
			.run_attempt(handoff.run_id())
			.expect("run attempt lookup should succeed")
			.expect("handoff attempt should exist")
			.status(),
		"succeeded"
	);
}

#[test]
fn manual_land_closeout_marker_roundtrips() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = init_git_checkout(&temp_dir, "repo");

	manual::write_manual_land_closeout_marker(
		&checkout,
		"https://github.com/hack-ink/decodex/pull/67",
		"deadbeef",
		"xy-225",
		r#"{"schema":"decodex/commit/1"}"#,
	)
	.expect("closeout marker should write");

	assert!(
		manual::manual_land_closeout_matches(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"deadbeef",
			"xy-225",
			r#"{"schema":"decodex/commit/1"}"#,
		)
		.expect("closeout marker should read"),
	);

	let marker = manual::read_manual_land_closeout_marker(&checkout)
		.expect("closeout marker should parse")
		.expect("closeout marker should exist");

	assert_eq!(marker.landed_change.as_deref(), Some(r#"{"schema":"decodex/commit/1"}"#));
	assert!(
		!checkout.join(".decodex/manual-land-closeout").exists(),
		"closeout marker should live under git admin state, not the working tree"
	);
}

#[test]
fn manual_land_closeout_marker_rejects_mismatched_receipts() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = init_git_checkout(&temp_dir, "repo");

	manual::write_manual_land_closeout_marker(
		&checkout,
		"https://github.com/hack-ink/decodex/pull/67",
		"deadbeef",
		"xy-225",
		r#"{"schema":"decodex/commit/1"}"#,
	)
	.expect("closeout marker should write");

	assert!(
		!manual::manual_land_closeout_matches(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"cafebabe",
			"xy-225",
			r#"{"schema":"decodex/commit/1"}"#,
		)
		.expect("closeout marker should compare"),
	);
}

#[test]
fn manual_land_handoff_lookup_prefers_current_branch_record() {
	let issue = sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_review_handoff_marker(
			"decodex",
			&issue.id,
			&ReviewHandoffMarker::new(
				String::from("run-current"),
				2,
				String::from("xy-225"),
				String::from("https://github.com/hack-ink/decodex/pull/67"),
				String::from("main"),
				String::from("xy-225"),
				String::from("deadbeef"),
			),
		)
		.expect("runtime handoff should persist");
	state_store
		.upsert_review_handoff_marker(
			"decodex",
			&issue.id,
			&ReviewHandoffMarker::new(
				String::from("run-other"),
				3,
				String::from("xy-225-next"),
				String::from("https://github.com/hack-ink/decodex/pull/99"),
				String::from("main"),
				String::from("xy-225-next"),
				String::from("cafebabe"),
			),
		)
		.expect("unrelated runtime handoff should persist");

	let handoff = manual::read_manual_land_handoff(&state_store, "decodex", &issue.id, "xy-225")
		.expect("manual land handoff should read")
		.expect("current branch handoff should be found");

	assert_eq!(handoff.branch_name(), "xy-225");
	assert_eq!(handoff.pr_url(), "https://github.com/hack-ink/decodex/pull/67");
}

#[test]
fn resolve_manual_config_path_uses_registered_project_for_linked_worktree() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = TestEnvVarGuard::set(
		"HOME",
		temp_dir.path().to_str().expect("temp dir path should be utf-8"),
	);
	let repo_root = temp_dir.path().join("target-repo");
	let worktree_root = repo_root.join(".worktrees");
	let config_dir = temp_dir.path().join(".codex/decodex/projects/pubfi");
	let config_path = config_dir.join("project.toml");

	fs::create_dir_all(&repo_root).expect("repo root should exist");
	fs::create_dir_all(&worktree_root).expect("worktree root should exist");
	fs::create_dir_all(&config_dir).expect("config dir should exist");

	assert!(
		test_support::hermetic_git_command()
			.args(["init", "-b", "main"])
			.current_dir(temp_dir.path())
			.arg(&repo_root)
			.status()
			.expect("git init should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.name", "Decodex Tests"])
			.current_dir(&repo_root)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "user.email", "decodex-tests@example.com"])
			.current_dir(&repo_root)
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.args(["config", "commit.gpgsign", "false"])
			.current_dir(&repo_root)
			.status()
			.expect("git config should run")
			.success()
	);

	fs::write(
		&config_path,
		format!(
			r#"
			service_id = "pubfi"

			[tracker]
			api_key_env_var = "HOME"

			[github]
			token_env_var = "PATH"

			[paths]
			repo_root = "{}"
			"#,
			repo_root.display()
		),
	)
	.expect("central project config should write");
	fs::write(config_dir.join("WORKFLOW.md"), "test workflow\n")
		.expect("central workflow should write");
	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");

	git_success(&repo_root, &["add", "README.md"]);
	git_success(&repo_root, &["commit", "-m", "bootstrap repo"]);

	let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let worktree = manager.ensure_worktree("XY-225", false).expect("worktree should create");
	let state_store = runtime::open_runtime_store().expect("state store should open");
	let canonical_config =
		fs::canonicalize(&config_path).expect("central config should canonicalize");

	runtime::register_project_config(&state_store, &config_path, true)
		.expect("central config should register");

	assert_eq!(
		manual::resolve_manual_config_path(None, &worktree.path)
			.expect("registered config path should resolve"),
		canonical_config
	);
}

#[test]
fn ensure_cli_repo_context_rejects_foreign_config_repo_root() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let current_repo = init_git_checkout(&temp_dir, "current-repo");
	let foreign_repo = init_git_checkout(&temp_dir, "foreign-repo");
	let config_path = foreign_repo.join("project.toml");

	fs::write(
		&config_path,
		r#"
			service_id = "pubfi"

			[tracker]
			api_key_env_var = "HOME"

			[github]
			token_env_var = "PATH"

			[paths]
			repo_root = "."
			"#,
	)
	.expect("foreign config should write");

	let config = ServiceConfig::from_path(&config_path).expect("config should parse");
	let canonical_repo_root =
		fs::canonicalize(&current_repo).expect("current repo root should canonicalize");
	let error = manual::ensure_cli_repo_context(&current_repo, &config, &canonical_repo_root)
		.expect_err("foreign config repo root should be rejected");

	assert!(error.to_string().contains("does not match loaded config repo root"));
	assert!(error.to_string().contains(&foreign_repo.display().to_string()));
}

fn sample_landing_state() -> PullRequestLandingState {
	PullRequestLandingState {
		url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		base_ref_name: String::from("release/1.x"),
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: String::from("XY-225"),
		head_ref_oid: String::from("deadbeef"),
		status_check_rollup_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: 0,
	}
}

fn sample_issue(
	id: &str,
	identifier: &str,
	labels_complete: bool,
	labels: &[&str],
) -> TrackerIssue {
	TrackerIssue {
		id: id.to_owned(),
		identifier: identifier.to_owned(),
		#[cfg(test)]
		project_slug: None,
		title: String::from("Sample issue"),
		author: None,
		description: String::from(""),
		priority: None,
		created_at: String::from("2026-04-13T00:00:00Z"),
		updated_at: String::from("2026-04-13T00:00:00Z"),
		state: TrackerState { id: String::from("state-1"), name: String::from("In Review") },
		team: TrackerTeam {
			id: String::from("team-1"),
			name: String::from("Core"),
			states: vec![TrackerState {
				id: String::from("state-1"),
				name: String::from("In Review"),
			}],
			labels: labels
				.iter()
				.enumerate()
				.map(|(index, label)| TrackerLabel {
					id: format!("team-label-{index}"),
					name: (*label).to_owned(),
				})
				.collect(),
		},
		labels_complete,
		labels: labels
			.iter()
			.enumerate()
			.map(|(index, label)| TrackerLabel {
				id: format!("issue-label-{index}"),
				name: (*label).to_owned(),
			})
			.collect(),
		blockers: Vec::new(),
	}
}
