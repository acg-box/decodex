use std::process::Command;

use crate::{
	orchestrator::{
		self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryQueue, StateStore, tests,
		tests::{
			FakePullRequestReviewStateInspector, FakeTracker, recovery_terminal_support,
			retry_scheduling::support,
		},
	},
	worktree::WorktreeManager,
};

#[test]
fn schedule_retry_after_child_exit_records_failure_retry_for_closeout_issue_after_tracker_completion()
 {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = support::sample_service_owned_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";
	let _path_guard = recovery_terminal_support::install_fake_merged_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);

	tests::seed_review_lifecycle_handoff_fixture(
		&state_store,
		config.service_id(),
		&issue.id,
		&worktree.branch_name,
		pr_url,
		&head_oid,
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");

	let inspector = FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]);

	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
			&tracker,
			&issue,
			&config,
			&workflow,
			&state_store,
			&inspector,
		)
		.expect("completed retained lane should pass closeout retention"),
		"completed closeout retries should only schedule when the retained PR lineage is already merged",
	);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 1"]).status().expect("failure exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		"In Review",
		IssueDispatchMode::Closeout,
		exit_status,
	)
	.expect("closeout failure retry should schedule after tracker completion");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);
	assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
	assert_eq!(entry.attempt, 1);
}

#[test]
fn schedule_retry_after_child_exit_keeps_blocked_closeout_retry_for_completed_issue_with_open_pr() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = support::sample_service_owned_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout-open-pr";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/176";
	let _path_guard = recovery_terminal_support::install_fake_open_pr_gh_response(
		&temp_dir, &worktree, pr_url, &head_oid,
	);

	tests::seed_review_lifecycle_handoff_fixture(
		&state_store,
		config.service_id(),
		&issue.id,
		&worktree.branch_name,
		pr_url,
		&head_oid,
	);

	let open_review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let inspector = FakePullRequestReviewStateInspector::new(vec![
		Ok(open_review_state.clone()),
		Ok(open_review_state),
	]);

	assert!(
		!orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
			&tracker,
			&issue,
			&config,
			&workflow,
			&state_store,
			&inspector,
		)
		.expect("open retained lane should not pass closeout dispatch"),
		"completed issues with an open PR must stay non-dispatchable",
	);
	assert_eq!(
		orchestrator::closeout_dispatch_block_reason_with_inspector(
			&tracker,
			&issue,
			&config,
			&workflow,
			&state_store,
			&inspector,
		)
		.expect("block reason lookup should succeed"),
		Some("pull_request_not_merged")
	);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 1"]).status().expect("failure exit should run");
	let mut retry_queue = RetryQueue::default();

	orchestrator::schedule_retry_after_child_exit(
		ChildExitRetryContext {
			retry_queue: &mut retry_queue,
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
		},
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		"In Review",
		IssueDispatchMode::Closeout,
		exit_status,
	)
	.expect("blocked closeout retry should stay queued after child exit");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);
	assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
	assert_eq!(entry.attempt, 1);
}
