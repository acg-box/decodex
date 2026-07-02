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

use crate::github::RepositoryContext;
use crate::test_support;
use crate::{
	manual::{self, ManualAuthority, ManualLandContext},
	pull_request::PullRequestLandingState,
	test_support::TestEnvVarGuard,
	tracker::{
		TrackerIssue, TrackerLabel, TrackerState, TrackerTeam,
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
	},
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

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

fn repo_root_manual_land_context(repo_root: &Path, worktree_root: &Path) -> ManualLandContext {
	ManualLandContext {
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
