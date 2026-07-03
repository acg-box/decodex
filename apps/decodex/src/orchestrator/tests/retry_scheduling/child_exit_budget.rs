use std::{process::Command, time::Duration};

use crate::{
	orchestrator::{
		self, ChildExitRetryContext, ChildRunRef, IssueDispatchMode, RetryQueue, StateStore, tests,
		tests::{
			FakePullRequestReviewStateInspector, FakeTracker, recovery_terminal_support,
			retry_scheduling::support,
		},
	},
	state,
	worktree::WorktreeManager,
};

#[test]
fn schedule_retry_after_child_exit_records_failure_retries_for_active_dispatch_modes() {
	for (issue_state, dispatch_mode, expected_dispatch_mode, run_id) in [
		("In Progress", IssueDispatchMode::Retry, IssueDispatchMode::Retry, "run-1"),
		(
			"In Review",
			IssueDispatchMode::ReviewRepair,
			IssueDispatchMode::ReviewRepair,
			"run-review-repair",
		),
	] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let issue = support::sample_service_owned_issue(issue_state);
		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let state_store = StateStore::open_in_memory().expect("state store should open");

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
			&issue.state.name,
			dispatch_mode,
			exit_status,
		)
		.expect("failure retry should schedule");

		let entry =
			retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

		assert_eq!(entry.dispatch_mode, expected_dispatch_mode);
		assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
		assert_eq!(entry.attempt, 1);
	}
}

#[test]
fn failure_retry_budget_ignores_prior_continuation_attempts() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-4";

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "succeeded")
		.expect("first continuation attempt should record");
	state_store
		.record_run_attempt("run-2", &issue.id, 2, "succeeded")
		.expect("second continuation attempt should record");
	state_store
		.record_run_attempt("run-3", &issue.id, 3, "succeeded")
		.expect("third continuation attempt should record");
	state_store
		.record_run_attempt(run_id, &issue.id, 4, "failed")
		.expect("first failure attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 4 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("first failure after continuations should still schedule");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
	assert_eq!(entry.attempt, 1);
	assert_eq!(
		orchestrator::retry_delay(entry.kind, entry.attempt, &workflow),
		Duration::from_millis(10_000)
	);
}

#[test]
fn schedule_retry_after_child_exit_terminalizes_exhausted_review_repair_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-review-repair-{attempt}"),
				&issue.id,
				attempt,
				"failed",
			)
			.expect("failed repair attempt should record");
	}

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
		ChildRunRef { issue_id: &issue.id, run_id: "run-review-repair-3", attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::ReviewRepair,
		exit_status,
	)
	.expect("exhausted review-repair child exit should terminalize");

	assert!(retry_queue.entries.is_empty(), "exhausted repair should not stay queued");
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[(issue.id.clone(), String::from("state-todo"))]
	);
	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-needs-attention")])]
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-active")])]
	);
	assert!(
		tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention")),
		"terminal failure comment should explain the exhausted repair"
	);
}

#[test]
fn schedule_retry_after_child_exit_counts_persisted_retry_budget_after_restart() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_retry_budget_attempt_count(&worktree.path, "previous-run", 2, 2)
		.expect("persisted retry budget marker should write");

	state_store
		.record_run_attempt("run-review-repair-3", &issue.id, 3, "failed")
		.expect("current failed repair attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id: "run-review-repair-3", attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::ReviewRepair,
		exit_status,
	)
	.expect("persisted retry budget should contribute to child-exit terminalization");

	assert!(retry_queue.entries.is_empty(), "exhausted repair should not stay queued");
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[(issue.id.clone(), String::from("state-todo"))]
	);
	assert_eq!(
		tracker.label_additions.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-needs-attention")])]
	);
	assert_eq!(
		tracker.label_removals.borrow().as_slice(),
		&[(issue.id.clone(), vec![String::from("label-active")])]
	);
}

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

	tests::seed_review_handoff_marker(
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

	tests::seed_review_handoff_marker(
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
