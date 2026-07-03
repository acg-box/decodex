use time::OffsetDateTime;

use crate::{
	agent::RUN_LEASE_IDLE_TIMEOUT,
	orchestrator::{
		self, ChildRunRef, CurrentChildRunContext, IssueDispatchMode, ReviewHandoffMarker,
		RunLeaseDisposition,
		tests::{self, FakeTracker, recovery_reconciliation::support},
	},
	state::{self, StateStore},
	worktree::WorktreeManager,
};

#[test]
fn run_lease_reconciliation_keeps_completed_closeout_lane_with_fresh_activity() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_ACTIVE_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = support::reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout-active";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/180";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.upsert_lease("pubfi", &issue.id, run_id, "Done").expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		OffsetDateTime::now_utc().unix_timestamp(),
	)
	.expect("run lease inspection should succeed");

	assert!(
		actions.is_empty(),
		"completed retained closeout lanes with fresh activity must not be reconciled as terminal or not-dispatchable"
	);
}

#[test]
fn active_daemon_child_reconciliation_keeps_completed_closeout_lane_with_fresh_activity() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_ACTIVE_DAEMON_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = support::reconciliation_sample_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-daemon-closeout-active";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/181";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store.upsert_lease("pubfi", &issue.id, run_id, "Done").expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	state::write_run_activity_marker(&worktree.path, run_id, 1)
		.expect("fresh activity marker should write");

	let actions = orchestrator::inspect_current_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Closeout,
		},
	)
	.expect("current daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"completed retained closeout daemon children with fresh activity must not be reconciled as terminal or not-dispatchable"
	);
}

#[test]
fn current_daemon_child_reconciliation_keeps_review_repair_lane_in_review() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = support::reconciliation_sample_service_owned_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-review-repair-current";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review-repair worktree should exist");

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Review")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let actions = orchestrator::inspect_current_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::ReviewRepair,
		},
	)
	.expect("current review-repair daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"review-repair lanes in In Review must stay current instead of being interrupted as not-dispatchable"
	);
}

#[test]
fn current_daemon_child_reconciliation_keeps_closeout_child_after_tracker_completion() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Done", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-closeout-completed";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "Done")
		.expect("lease should record");

	let actions = orchestrator::inspect_current_daemon_child_reconciliation(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Closeout,
		},
	)
	.expect("current closeout daemon-child inspection should succeed");

	assert!(
		actions.is_empty(),
		"closeout children may legitimately observe a completed tracker issue while they finish local cleanup"
	);
}

#[test]
fn run_lease_reconciliation_treats_completed_retained_handoff_as_success() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue_with_sort_fields(
		"issue-handoff-complete",
		"PUB-205",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/205";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("stale run activity should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("run lease inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(actions[0].disposition, RunLeaseDisposition::RetainedReviewComplete));

	orchestrator::apply_run_lease_reconciliation(
		&tracker,
		&config,
		&state_store,
		&worktree_manager,
		actions,
	)
	.expect("completed retained handoff reconciliation should succeed");

	assert!(state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none());
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"retained post-review worktree must stay available for merge/closeout"
	);
	assert_eq!(
		state_store
			.run_attempt(run_id)
			.expect("run attempt lookup should succeed")
			.expect("run attempt should exist")
			.status(),
		"succeeded"
	);
	assert!(
		tracker.comments.borrow().iter().all(|comment| !comment.contains("stalled_run_detected")),
		"completed retained handoff must not be routed through needs-attention"
	);
}

#[test]
fn run_lease_reconciliation_ignores_stale_retained_handoff_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue_with_sort_fields(
		"issue-stale-handoff",
		"PUB-205B",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-current";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("stale run activity should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&ReviewHandoffMarker::new(
			"run-previous",
			1,
			&worktree.branch_name,
			"https://github.com/hack-ink/decodex/pull/205",
			"main",
			&worktree.branch_name,
			&head_oid,
		),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_run_lease_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		None,
		now,
	)
	.expect("run lease inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(
		actions[0].disposition,
		RunLeaseDisposition::Stalled { idle_for }
			if idle_for >= RUN_LEASE_IDLE_TIMEOUT
	));
}

#[test]
fn active_daemon_child_reconciliation_treats_completed_retained_handoff_as_success() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue_with_sort_fields(
		"issue-daemon-handoff-complete",
		"PUB-206",
		"In Progress",
		&[],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let run_id = "run-1";
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/206";

	state_store
		.record_run_attempt(run_id, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(config.service_id(), &issue.id, run_id, "In Progress")
		.expect("lease should record");
	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");
	state_store
		.append_event(run_id, 1, "thread/status/changed", "{\"status\":\"active\"}")
		.expect("stale run activity should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let now =
		OffsetDateTime::now_utc().unix_timestamp() + RUN_LEASE_IDLE_TIMEOUT.as_secs() as i64 + 1;
	let actions = orchestrator::inspect_current_daemon_child_reconciliation_at(
		&tracker,
		&config,
		&workflow,
		&state_store,
		CurrentChildRunContext {
			child: ChildRunRef { issue_id: &issue.id, run_id, attempt_number: 1 },
			workflow: &workflow,
			dispatch_mode: IssueDispatchMode::Normal,
		},
		now,
	)
	.expect("current daemon-child inspection should succeed");

	assert_eq!(actions.len(), 1);
	assert!(matches!(actions[0].disposition, RunLeaseDisposition::RetainedReviewComplete));
}
