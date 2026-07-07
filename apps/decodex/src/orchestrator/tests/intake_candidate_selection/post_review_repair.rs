use crate::{
	orchestrator::{
		self, IssueDispatchMode, TargetIssueRunContext,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker, TEST_SERVICE_ID,
			intake_candidate_selection::support,
		},
	},
	state::{self, StateStore},
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn plan_project_issue_run_prefers_post_review_repair_lane_over_normal_candidate() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let normal_issue = tests::sample_issue_with_sort_fields(
		"issue-1",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let repair_issue = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"In Review",
		&[tracker::automation_active_label(TEST_SERVICE_ID).as_str()],
		Some(3),
		"2026-03-13T04:18:17.133Z",
	);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![normal_issue.clone(), repair_issue.clone()],
		vec![vec![repair_issue.clone()], vec![repair_issue.clone()], vec![repair_issue.clone()]],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&repair_issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let inspector = FakePullRequestReviewStateInspector::new(vec![Ok(
		tests::sample_pull_request_review_state(
			pr_url,
			&worktree.branch_name,
			&head_oid,
			Some("CHANGES_REQUESTED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		),
	)]);
	let selected = orchestrator::select_post_review_repair_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review repair selection should succeed")
	.expect("repair lane should be selected");

	assert_eq!(selected.identifier, repair_issue.identifier);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![repair_issue.clone()],
		vec![vec![repair_issue.clone()], vec![repair_issue.clone()]],
	);
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &repair_issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("targeted review-repair planning should succeed")
	.expect("review-repair issue run should plan");

	assert_eq!(summary.issue_identifier, repair_issue.identifier);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::ReviewRepair);
	assert_eq!(summary.issue_state, "In Review");
}

#[test]
fn post_review_repair_selection_skips_exhausted_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let repair_issue = support::candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![repair_issue.clone()],
		vec![vec![repair_issue.clone()]],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-review-repair-{attempt}"),
				&repair_issue.id,
				attempt,
				"failed",
			)
			.expect("failed repair attempt should record");
	}

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&repair_issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let inspector = FakePullRequestReviewStateInspector::new(vec![Ok(
		tests::sample_pull_request_review_state(
			pr_url,
			&worktree.branch_name,
			&head_oid,
			Some("CHANGES_REQUESTED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		),
	)]);
	let selected = orchestrator::select_post_review_repair_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review repair selection should succeed");

	assert!(selected.is_none(), "exhausted repair lanes should not be redispatched");
}

#[test]
fn targeted_post_review_repair_skips_persisted_exhausted_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let repair_issue = support::candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![repair_issue.clone()],
		vec![vec![repair_issue.clone()], vec![repair_issue.clone()]],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&repair_issue.identifier, false)
		.expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&repair_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	state::write_run_retry_budget_attempt_count(&worktree.path, "older-run", 3, 3)
		.expect("retry budget marker should write");

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &repair_issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("targeted review-repair planning should succeed");

	assert!(summary.is_none(), "persisted exhausted budget should block direct repair dispatch");
}

#[test]
fn targeted_retry_blocks_retained_review_lifecycle_authority_in_state_transition_window() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("In Progress", &[active_label.as_str()]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

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
	.expect("targeted retry planning should succeed");

	assert!(
		summary.is_none(),
		"retry dispatch must not mint a duplicate attempt for a retained review handoff lane"
	);
}
