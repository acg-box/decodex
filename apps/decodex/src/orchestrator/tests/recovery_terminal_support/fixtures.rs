use tempfile::TempDir;

#[rustfmt::skip]
use crate::config::ServiceConfig;
#[rustfmt::skip]
use crate::orchestrator::tests::{self, TEST_SERVICE_ID};
#[rustfmt::skip]
use crate::orchestrator::{self, IssueDispatchMode, ReviewLifecycleHandoffFixture};
#[rustfmt::skip]
use crate::state::StateStore;
#[rustfmt::skip]
use crate::test_support::{self, TestEnvVarGuard};
#[rustfmt::skip]
use crate::tracker::{self, TrackerIssue, TrackerState};
#[rustfmt::skip]
use crate::workflow::WorkflowDocument;
#[rustfmt::skip]
use crate::worktree::{WorktreeManager, WorktreeSpec};
use crate::orchestrator::tests::recovery_terminal_support::{self};
pub(in crate::orchestrator::tests) struct CloseoutIdentityFixture {
	pub(in crate::orchestrator::tests) _temp_dir: TempDir,
	pub(in crate::orchestrator::tests) _path_guard: TestEnvVarGuard,
	pub(in crate::orchestrator::tests) config: ServiceConfig,
	pub(in crate::orchestrator::tests) workflow: WorkflowDocument,
	pub(in crate::orchestrator::tests) tracker: tests::FakeTracker,
	pub(in crate::orchestrator::tests) state_store: StateStore,
	pub(in crate::orchestrator::tests) issue: TrackerIssue,
	pub(in crate::orchestrator::tests) worktree: WorktreeSpec,
	pub(in crate::orchestrator::tests) pr_url: String,
	pub(in crate::orchestrator::tests) head_oid: String,
	pub(in crate::orchestrator::tests) completed_run_id: String,
}

pub(in crate::orchestrator::tests) fn sample_active_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

pub(in crate::orchestrator::tests) fn sample_active_issue_without_needs_attention_team_label(
	state_name: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_without_needs_attention_team_label(state_name, &[active_label.as_str()])
}
pub(in crate::orchestrator::tests) fn issue_with_completed_state(
	mut issue: TrackerIssue,
) -> TrackerIssue {
	if !issue.team.states.iter().any(|state| state.name == "Done") {
		issue
			.team
			.states
			.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });
	}

	issue
}

pub(in crate::orchestrator::tests) fn sample_closeout_issue_run(
	issue: &TrackerIssue,
	worktree: &WorktreeSpec,
	run_id: &str,
) -> orchestrator::IssueRunPlan {
	orchestrator::IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: worktree.branch_name.clone(),
			issue_identifier: issue.identifier.clone(),
			path: worktree.path.clone(),
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		attempt_number: 1,
		run_id: String::from(run_id),
		retry_budget_base: 0,
	}
}

pub(in crate::orchestrator::tests) fn closeout_identity_fixture() -> CloseoutIdentityFixture {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue =
		issue_with_completed_state(tests::sample_issue("In Review", &[active_label.as_str()]));
	let tracker = tests::FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()]; 8],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = String::from("https://github.com/hack-ink/decodex/pull/703");
	let _path_guard = recovery_terminal_support::install_fake_closeout_gh_responses(
		&temp_dir, &worktree, &pr_url, &head_oid,
	);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");
	let completed_run_id = String::from("pub-703-attempt-1-111");

	recovery_terminal_support::initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	recovery_terminal_support::route_origin_github_url_to_local_bare_repo(
		config.repo_root(),
		&remote_root,
	);

	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);

	state_store
		.upsert_review_lifecycle_handoff_fixture(
			config.service_id(),
			&issue.id,
			&ReviewLifecycleHandoffFixture::new(
				&completed_run_id,
				1,
				&worktree.branch_name,
				&pr_url,
				"main",
				&worktree.branch_name,
				&head_oid,
			),
		)
		.expect("review lifecycle handoff fixture should persist");
	state_store
		.record_run_attempt(&completed_run_id, &issue.id, 1, "succeeded")
		.expect("completed handoff attempt should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("retained closeout worktree should record");

	CloseoutIdentityFixture {
		_temp_dir: temp_dir,
		_path_guard,
		config,
		workflow,
		tracker,
		state_store,
		issue,
		worktree,
		pr_url,
		head_oid,
		completed_run_id,
	}
}
