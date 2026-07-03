use std::{
	fs,
	path::Path,
	process::Command,
	thread,
	time::{Duration, Instant},
};

use time::OffsetDateTime;

use crate::{
	orchestrator::{
		self, CONTINUATION_PENDING_RUN_STATUS, ChildExitRetryContext, ChildRunRef, DaemonRunChild,
		DaemonTickRuntimeContext, IssueDispatchMode, PullRequestReviewState, RetryDispatchDecision,
		RetryEntry, RetryEntryLifecycle, RetryKind, RetryQueue, ReviewLevel, StateStore,
		TERMINAL_GUARDED_RUN_STATUS, TargetIssueRunContext, tests,
		tests::{
			FakePullRequestReviewStateInspector, FakeTracker, TEST_SERVICE_ID,
			recovery_terminal_support,
		},
	},
	state,
	tracker::{self, TrackerIssue},
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

const PUB_704_RETAINED_HEAD_SUBJECT: &str =
	r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-704"}"#;
const PUB_704_RETAINED_LANDED_SUBJECT: &str = r#"{"schema":"decodex/commit/1","summary":"Land current retained handoff","authority":"PUB-704"}"#;

fn sample_approved_clean_review_state(
	pr_url: &str,
	branch_name: &str,
	head_oid: &str,
) -> PullRequestReviewState {
	tests::sample_pull_request_review_state(
		pr_url,
		branch_name,
		head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	)
}

fn sample_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

fn sample_service_owned_issue_without_needs_attention_team_label(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_without_needs_attention_team_label(state_name, &[active_label.as_str()])
}

fn sample_service_owned_issue_with_project_slug_and_sort_fields(
	id: &str,
	identifier: &str,
	project_slug: &str,
	state_name: &str,
	sort_value: Option<i64>,
	updated_at: &str,
) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue_with_project_slug_and_sort_fields(
		id,
		identifier,
		project_slug,
		state_name,
		&[active_label.as_str()],
		sort_value,
		updated_at,
	)
}

#[test]
fn retry_delay_distinguishes_continuation_and_capped_failure_backoff() {
	let (_, _, workflow) = tests::temp_project_layout();

	assert_eq!(
		orchestrator::retry_delay(orchestrator::RetryKind::Continuation, 1, &workflow,),
		Duration::from_millis(1_000)
	);
	assert_eq!(
		orchestrator::retry_delay(orchestrator::RetryKind::Failure, 1, &workflow),
		Duration::from_millis(10_000)
	);
	assert_eq!(
		orchestrator::retry_delay(orchestrator::RetryKind::Failure, 10, &workflow),
		Duration::from_millis(300_000)
	);
}

#[test]
fn retry_run_dry_run_enforces_active_ownership() {
	for (case_name, issue, expected_dispatch) in [
		("active issue", sample_service_owned_issue("In Progress"), true),
		("unowned issue", tests::sample_issue("In Progress", &[]), false),
	] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let tracker = FakeTracker::with_refresh_snapshots(
			vec![issue.clone()],
			vec![vec![issue.clone()], vec![issue.clone()]],
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_id: &issue.id,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			dry_run: true,
			lease_preacquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			dispatch_mode: IssueDispatchMode::Retry,
			preferred_run_identity: None,
			preferred_retry_budget_base: None,
		})
		.expect("retry run should succeed");

		assert_eq!(summary.is_some(), expected_dispatch, "{case_name}");
	}
}

#[test]
fn targeted_run_dry_run_accepts_startable_issue_with_normal_dispatch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Normal,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("targeted run should succeed");

	assert!(summary.is_some(), "normal targeted dispatch should accept startable issues");
}

#[test]
fn retry_run_dry_run_rejects_terminal_guarded_issue_without_attention_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue_without_needs_attention_team_label("In Progress");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("run-1", &issue.id, 1, TERMINAL_GUARDED_RUN_STATUS)
		.expect("terminal guard attempt should record");

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Retry,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("retry run should succeed");

	assert!(
		summary.is_none(),
		"retry should reject issues that remain in progress only as a terminal guard"
	);
}

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
		let issue = sample_service_owned_issue(issue_state);
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
	let issue = sample_service_owned_issue("In Progress");
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
	let issue = sample_service_owned_issue("In Review");
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
	let issue = sample_service_owned_issue("In Review");
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
	let issue = sample_service_owned_issue("Done");
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
	let issue = sample_service_owned_issue("Done");
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

#[test]
fn future_review_repair_retry_keeps_backoff_window_until_due() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::ReviewRepair,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		kind: RetryKind::Failure,
		attempt: 1,
		ready_at: Instant::now() + Duration::from_secs(60),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("future review-repair retry should stay queued");

	assert!(matches!(
		decision,
		RetryDispatchDecision::Blocked{ excluded_issue_ids }
			if excluded_issue_ids == vec![issue.id.clone()]
	));
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"review-repair retries should keep their queued backoff window until ready"
	);
	assert_eq!(
		tracker.refresh_snapshots.borrow().len(),
		1,
		"future review-repair retry planning should not refresh tracker state before the retry is due"
	);
}

#[test]
fn due_review_repair_retry_drops_after_backoff_budget_exhausted() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(&format!("run-{attempt}"), &issue.id, attempt, "failed")
			.expect("failed repair attempt should record");
	}

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::ReviewRepair,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		kind: RetryKind::Failure,
		attempt: 3,
		ready_at: Instant::now(),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("exhausted review-repair retry should be dropped");

	assert!(matches!(decision, RetryDispatchDecision::Continue));
	assert!(
		retry_queue.entries.is_empty(),
		"exhausted review-repair retry should not hold the queued claim"
	);
}

#[test]
fn due_review_repair_retry_drops_when_active_ownership_is_gone() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::ReviewRepair,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		kind: RetryKind::Failure,
		attempt: 1,
		ready_at: Instant::now(),
	});

	let decision = orchestrator::plan_due_retry_run(
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("review-repair retry planning should succeed");

	assert!(matches!(decision, RetryDispatchDecision::Continue));
	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"review-repair retries should be dropped when active ownership is gone"
	);
}

#[test]
fn interrupted_exits_consume_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-3";

	state_store
		.record_run_attempt("run-1", &issue.id, 1, "interrupted")
		.expect("first interrupted attempt should record");
	state_store
		.record_run_attempt("run-2", &issue.id, 2, "interrupted")
		.expect("second interrupted attempt should record");
	state_store
		.record_run_attempt(run_id, &issue.id, 3, "interrupted")
		.expect("third interrupted attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("retry scheduling should succeed");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"interrupted exits should exhaust the retry budget"
	);
}

#[test]
fn schedule_retry_after_child_exit_records_continuation_retry_for_clean_exit() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, CONTINUATION_PENDING_RUN_STATUS)
		.expect("run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 0"]).status().expect("success exit should run");
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
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("continuation retry should schedule");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 1)
		.expect("private continuation lineage events should load");

	assert_eq!(entry.kind, orchestrator::RetryKind::Continuation);
	assert_eq!(entry.attempt, 1);
	assert!(events.iter().any(|event| {
		event.event_type() == "continuation_lineage"
			&& event.payload()["continuation_of_run_id"] == run_id
			&& event.payload()["retry_budget_consumed"] == false
			&& event.payload()["next_retry_kind"] == "continuation"
	}));
}

#[test]
fn schedule_retry_after_child_exit_terminalizes_open_phase_goal_tracked_rewrites() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout_with_workflow_markdown(
		&tests::sample_workflow_markdown("pubfi", &[], "Phase goal validation policy.\n", 1)
			.replace(
				"canonicalize_commands = []",
				"canonicalize_commands = [\"printf 'rewritten\\\\n' > other.txt\"]",
			),
	);
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-3";

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	tests::commit_worktree_change(config.repo_root(), "other.txt", "before\n", "add other file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	for (attempt, recorded_run_id) in [(1, "run-1"), (2, "run-2"), (3, run_id)] {
		state_store
			.record_run_attempt(recorded_run_id, &issue.id, attempt, "failed")
			.expect("failed run attempt should record");
	}

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&config.repo_root().display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("phase goal event should record");

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
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("open phase goal tracked rewrites should terminalize cleanly");

	let run_attempt = state_store
		.run_attempt(run_id)
		.expect("run attempt lookup should succeed")
		.expect("run attempt should still exist");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 3)
		.expect("private events should load");
	let comments = tracker.comments.borrow();

	assert!(!retry_queue.entries.contains_key(&issue.id));
	assert_eq!(run_attempt.status(), "failed");
	assert!(events.iter().any(|event| {
		event.event_type() == "phase_goal_transition"
			&& event.payload()["signal"] == "validation_fail"
			&& event.payload()["payload"]["disposition"] == "needs_human_attention"
			&& event.payload()["payload"]["trackedRewrites"]["owned"] == false
	}));
	assert!(events.iter().all(|event| event.event_type() != "phase_goal_recovery"));
	assert!(comments.iter().any(|comment| {
		comment.contains("decodex retained partial progress and needs attention")
			&& comment.contains("partial_progress_retained")
			&& comment.contains("Source failure class `repo_gate_tracked_rewrites_left`")
	}));
}

#[test]
fn schedule_retry_after_child_exit_respects_terminal_finalize_before_phase_goal_recovery() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-3";

	tests::commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");
	fs::write(config.repo_root().join("ready.txt"), "after\n").expect("tracked diff should write");

	for (attempt, recorded_run_id) in [(1, "run-1"), (2, "run-2"), (3, run_id)] {
		state_store
			.record_run_attempt(recorded_run_id, &issue.id, attempt, "failed")
			.expect("failed run attempt should record");
	}

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&config.repo_root().display().to_string(),
		)
		.expect("worktree should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"phase_goal_set",
			serde_json::json!({
				"schema": "decodex.phase_goal_signal/1",
				"phase": "implement_to_validation_ready",
				"payload": {
					"phase": "implement_to_validation_ready",
					"status": "active",
				},
			}),
		)
		.expect("phase goal event should record");
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			&issue.id,
			run_id,
			3,
			"terminal_finalize",
			serde_json::json!({
				"path": "manual_attention",
				"mode": "normal",
				"branch": "x/pubfi-pub-101",
				"worktree_path": config.repo_root().display().to_string(),
			}),
		)
		.expect("terminal finalize event should record");

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
		ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 3 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("terminalized child exit should keep the terminal path");

	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, run_id, 3)
		.expect("private events should load");

	assert!(!retry_queue.entries.contains_key(&issue.id));
	assert!(
		events.iter().all(|event| event.event_type() != "phase_goal_recovery"),
		"terminal finalize intent must not be replaced by active phase-goal recovery"
	);
}

#[test]
fn schedule_retry_after_child_exit_preserves_specific_retry_schedule_kind_for_failure_retry() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_path = config.worktree_root().join("PUB-101");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "failed")
		.expect("run attempt should record");
	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");
	state::write_run_retry_schedule(
		&worktree_path,
		run_id,
		1,
		"git_lock_contention",
		OffsetDateTime::now_utc().unix_timestamp() + 30,
	)
	.expect("specific retry schedule should write");

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
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("failure retry should schedule");

	let marker = state::read_run_activity_marker_snapshot(&worktree_path)
		.expect("retry schedule should remain readable")
		.expect("retry marker should exist");
	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Failure);
	assert_eq!(marker.retry_kind(), Some("git_lock_contention"));
}

#[test]
fn schedule_retry_after_child_exit_retains_continuation_retry_for_stale_startable_issue() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("Todo");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, CONTINUATION_PENDING_RUN_STATUS)
		.expect("run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 0"]).status().expect("success exit should run");
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
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("continuation retry should tolerate a stale startable tracker reread");

	let entry = retry_queue.entries.get(&issue.id).expect("retry entry should exist for the issue");

	assert_eq!(entry.kind, orchestrator::RetryKind::Continuation);
	assert_eq!(entry.attempt, 1);
}

#[test]
fn schedule_retry_after_child_exit_skips_retry_for_completed_successful_run() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "succeeded")
		.expect("completed run attempt should record");

	let exit_status =
		Command::new("sh").args(["-c", "exit 0"]).status().expect("success exit should run");
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
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("completed successful runs should not schedule another retry");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"successful review-handoff style exits must not reopen the same run as a continuation"
	);
}

#[test]
fn schedule_retry_after_child_exit_requires_exact_run_id() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.record_run_attempt("other-run", &issue.id, 1, "running")
		.expect("other run attempt should record");

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
		ChildRunRef { issue_id: &issue.id, run_id: "planned-run", attempt_number: 1 },
		issue.project_slug.as_deref().expect("sample issue should carry a project slug"),
		&issue.state.name,
		IssueDispatchMode::Retry,
		exit_status,
	)
	.expect("retry scheduling should succeed");

	assert!(
		!retry_queue.entries.contains_key(&issue.id),
		"retry scheduling should ignore a different run that only matches the issue and attempt"
	);
}

#[test]
fn exited_retry_child_keeps_queued_claim_when_no_run_attempt_was_persisted() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let mut child =
		Command::new("sh").args(["-c", "exit 1"]).spawn().expect("child process should spawn");

	for _ in 0..20 {
		if child.try_wait().expect("child status should query").is_some() {
			break;
		}

		thread::sleep(Duration::from_millis(10));
	}

	let mut active_children = vec![orchestrator::DaemonRunChild {
		child,
		issue_id: issue.id.clone(),
		run_id: String::from("planned-run"),
		attempt_number: 1,
		initial_issue_state: issue.state.name.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: orchestrator::IssueDispatchMode::Retry,
		from_retry_queue: true,
		workflow: workflow.clone(),
	}];
	let mut retry_queue = RetryQueue::default();

	retry_queue.upsert(RetryEntry {
		issue_id: issue.id.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		continuation_initial_issue_state: None,
		lifecycle: RetryEntryLifecycle::Active,
		dispatch_mode: IssueDispatchMode::Retry,
		kind: RetryKind::Failure,
		attempt: 1,
		ready_at: Instant::now(),
	});

	orchestrator::inspect_or_clear_active_children(
		&mut active_children,
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("exited child cleanup should succeed");

	assert!(active_children.is_empty(), "exited child should be cleared");
	assert!(
		retry_queue.entries.contains_key(&issue.id),
		"retry claim should remain queued when the child exits before persisting a run attempt"
	);
}

#[test]
fn exited_successful_child_marks_recent_run_succeeded_before_cleanup() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let run_id = "planned-run";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");

	let mut child =
		Command::new("sh").args(["-c", "exit 0"]).spawn().expect("child process should spawn");

	for _ in 0..20 {
		if child.try_wait().expect("child status should query").is_some() {
			break;
		}

		thread::sleep(Duration::from_millis(10));
	}

	let mut active_children = vec![orchestrator::DaemonRunChild {
		child,
		issue_id: issue.id.clone(),
		run_id: String::from(run_id),
		attempt_number: 1,
		initial_issue_state: issue.state.name.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: orchestrator::IssueDispatchMode::Retry,
		from_retry_queue: false,
		workflow: workflow.clone(),
	}];
	let mut retry_queue = RetryQueue::default();

	orchestrator::inspect_or_clear_active_children(
		&mut active_children,
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("exited child cleanup should succeed");

	assert!(active_children.is_empty(), "exited child should be cleared");
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain recorded")
			.status(),
		"succeeded"
	);
}

#[test]
fn exited_unsuccessful_child_does_not_downgrade_persisted_success() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = sample_service_owned_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let run_id = "planned-run";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "succeeded")
		.expect("run attempt should record completed child outcome");

	let mut child =
		Command::new("sh").args(["-c", "exit 1"]).spawn().expect("child process should spawn");

	for _ in 0..20 {
		if child.try_wait().expect("child status should query").is_some() {
			break;
		}

		thread::sleep(Duration::from_millis(10));
	}

	let mut active_children = vec![orchestrator::DaemonRunChild {
		child,
		issue_id: issue.id.clone(),
		run_id: String::from(run_id),
		attempt_number: 1,
		initial_issue_state: issue.state.name.clone(),
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: orchestrator::IssueDispatchMode::Retry,
		from_retry_queue: false,
		workflow: workflow.clone(),
	}];
	let mut retry_queue = RetryQueue::default();

	orchestrator::inspect_or_clear_active_children(
		&mut active_children,
		&mut retry_queue,
		&tracker,
		&config,
		&workflow,
		&state_store,
		&worktree_manager,
	)
	.expect("exited child cleanup should succeed");

	assert!(active_children.is_empty(), "exited child should be cleared");
	assert!(
		retry_queue.entries.is_empty(),
		"persisted success should not schedule a retry after a late wrapper failure"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain recorded")
			.status(),
		"succeeded"
	);
}

fn assert_fake_admin_merge_invocation_present(
	invocation_log_path: &Path,
	head_oid: &str,
	merge_subject: &str,
	pr_url: &str,
) {
	let gh_invocation_log =
		fs::read_to_string(invocation_log_path).expect("fake gh invocation log should read");
	let expected_invocation = [
		"pr",
		"merge",
		"--admin",
		"--merge",
		"--match-head-commit",
		head_oid,
		"--subject",
		merge_subject,
		"--body",
		"",
		pr_url,
	]
	.join("\n");

	assert!(
		gh_invocation_log.contains(&expected_invocation),
		"fake gh invocation log should contain the admin merge command"
	);
}

fn stop_daemon_children(active_children: &mut [DaemonRunChild]) {
	for daemon_child in active_children {
		let _ = daemon_child.child.kill();
		let _ = daemon_child.child.wait();
	}
}

fn spawn_sleeping_daemon_child(
	active_issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> DaemonRunChild {
	let child =
		Command::new("sh").args(["-c", "sleep 30"]).spawn().expect("child process should spawn");

	DaemonRunChild {
		child,
		issue_id: active_issue.id.clone(),
		run_id: String::from("leased-run"),
		attempt_number: 1,
		initial_issue_state: active_issue.state.name.clone(),
		retry_project_slug: active_issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Normal,
		from_retry_queue: false,
		workflow: workflow.clone(),
	}
}

#[test]
fn daemon_tick_reconciles_ready_retained_review_lane_before_dry_run_planning() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&base_config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let active_issue = sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-active",
		"PUB-200",
		TEST_SERVICE_ID,
		"In Progress",
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let retained_issue = sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-retained",
		"PUB-704",
		TEST_SERVICE_ID,
		"In Review",
		Some(2),
		"2026-03-13T04:18:17.133Z",
	);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![active_issue.clone(), retained_issue.clone()],
		vec![vec![active_issue.clone()], vec![retained_issue.clone()]],
	);
	let retained_worktree = worktree_manager
		.ensure_worktree(&retained_issue.identifier, false)
		.expect("retained worktree should exist");
	let pr_url = "https://github.com/hack-ink/decodex/pull/704";
	let head_oid = tests::commit_worktree_change(
		&retained_worktree.path,
		"retained.txt",
		"ready\n",
		PUB_704_RETAINED_HEAD_SUBJECT,
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&retained_issue.id,
			&retained_worktree.branch_name,
			&retained_worktree.path.display().to_string(),
		)
		.expect("retained worktree should record");
	state_store
		.record_run_attempt("leased-run", &active_issue.id, 1, "running")
		.expect("current lane should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&retained_issue.id,
		&tests::sample_review_handoff_marker(&retained_worktree.branch_name, pr_url, &head_oid),
	);

	let review_state = tests::sample_pull_request_review_state(
		pr_url,
		&retained_worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
	let mut active_children = vec![spawn_sleeping_daemon_child(&active_issue, &workflow)];
	let mut retry_queue = RetryQueue::default();
	let result = orchestrator::run_daemon_tick_with_review_state_inspector(
		&tests::service_config_path(config.repo_root()),
		&state_store,
		&mut active_children,
		&mut retry_queue,
		DaemonTickRuntimeContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			worktree_manager: &worktree_manager,
			review_state_inspector: &FakePullRequestReviewStateInspector::new(vec![Ok(
				review_state,
			)]),
			recoverable_worktree_skip_cache: None,
		},
	);

	stop_daemon_children(&mut active_children);

	result.expect("daemon tick should reconcile retained review lanes");

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&retained_worktree.path,
	);

	assert_eq!(marker.phase(), "waiting_for_merge");

	assert_fake_admin_merge_invocation_present(
		&invocation_log_path,
		&head_oid,
		PUB_704_RETAINED_LANDED_SUBJECT,
		pr_url,
	);
}

#[test]
fn daemon_tick_clears_terminal_mapping_without_worktree_before_retained_land() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let (gh_command_path, invocation_log_path) =
		tests::install_fake_admin_merge_gh_response(&temp_dir);
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var_and_command_path(
			&base_config,
			"PATH",
			&gh_command_path,
		),
		ReviewLevel::Standard,
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let mut terminal_issue = sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-terminal",
		"PUB-703",
		TEST_SERVICE_ID,
		"Done",
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);

	terminal_issue.labels.clear();
	terminal_issue.team.labels.clear();

	let retained_issue = sample_service_owned_issue_with_project_slug_and_sort_fields(
		"issue-retained",
		"PUB-704",
		TEST_SERVICE_ID,
		"In Review",
		Some(2),
		"2026-03-13T04:18:17.133Z",
	);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![terminal_issue.clone(), retained_issue.clone()],
		vec![
			vec![terminal_issue.clone(), retained_issue.clone()],
			vec![terminal_issue.clone(), retained_issue.clone()],
			vec![retained_issue.clone()],
		],
	);
	let retained_worktree = worktree_manager
		.ensure_worktree(&retained_issue.identifier, false)
		.expect("retained worktree should exist");
	let pr_url = "https://github.com/hack-ink/decodex/pull/704";
	let head_oid = tests::commit_worktree_change(
		&retained_worktree.path,
		"retained.txt",
		"ready\n",
		PUB_704_RETAINED_HEAD_SUBJECT,
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&terminal_issue.id,
			"x/pubfi-pub-703",
			&config.worktree_root().join("PUB-703").display().to_string(),
		)
		.expect("terminal stale worktree should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&retained_issue.id,
			&retained_worktree.branch_name,
			&retained_worktree.path.display().to_string(),
		)
		.expect("retained worktree should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&retained_issue.id,
		&tests::sample_review_handoff_marker(&retained_worktree.branch_name, pr_url, &head_oid),
	);

	let mut active_children = Vec::new();
	let mut retry_queue = RetryQueue::default();

	orchestrator::run_daemon_tick_with_review_state_inspector(
		&tests::service_config_path(config.repo_root()),
		&state_store,
		&mut active_children,
		&mut retry_queue,
		DaemonTickRuntimeContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			worktree_manager: &worktree_manager,
			review_state_inspector: &FakePullRequestReviewStateInspector::new(vec![Ok(
				sample_approved_clean_review_state(
					pr_url,
					&retained_worktree.branch_name,
					&head_oid,
				),
			)]),
			recoverable_worktree_skip_cache: None,
		},
	)
	.expect("daemon tick should not fail on stale terminal worktree state");

	assert!(
		state_store
			.worktree_for_issue(&terminal_issue.id)
			.expect("terminal worktree lookup should succeed")
			.is_none(),
		"terminal mapping without a local worktree should be cleared"
	);

	assert_fake_admin_merge_invocation_present(
		&invocation_log_path,
		&head_oid,
		PUB_704_RETAINED_LANDED_SUBJECT,
		pr_url,
	);
}
