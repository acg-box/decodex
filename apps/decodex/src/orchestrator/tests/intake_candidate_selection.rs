use std::{cell::RefCell, fs, path::Path};

use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, IssueDispatchMode, RetainedReviewRunIdentity, ReviewHandoffMarker, RunSummary,
		TERMINAL_GUARDED_RUN_STATUS, TargetIssueRunContext,
		tests::{self, FakePullRequestReviewStateInspector, FakeTracker, TEST_SERVICE_ID},
	},
	state::{self, StateStore},
	tracker::{self, TrackerIssue},
	workflow::WorkflowDocument,
	worktree::WorktreeManager,
};

fn candidate_selection_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

fn sample_handoff_summary(issue: &TrackerIssue, worktree_path: &Path) -> RunSummary {
	RunSummary {
		project_id: String::from("pubfi"),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: String::from("In Progress"),
		initial_issue_state: String::from("Todo"),
		retry_project_slug: String::from("pubfi"),
		dispatch_mode: IssueDispatchMode::Normal,
		branch_name: String::from("main"),
		worktree_path: worktree_path.to_path_buf(),
		attempt_number: 1,
		run_id: String::from("run-review-handoff"),
		continuation_pending: false,
	}
}

#[test]
fn candidate_selection_sorts_by_priority_created_at_and_identifier() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let high_priority = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:18:17.133Z",
	);
	let oldest_same_priority = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-103",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:15:17.133Z",
	);
	let newest_same_priority = tests::sample_issue_with_sort_fields(
		"issue-4",
		"PUB-104",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:19:17.133Z",
	);
	let no_priority = tests::sample_issue_with_sort_fields(
		"issue-5",
		"PUB-105",
		"Todo",
		&[],
		None,
		"2026-03-13T04:14:17.133Z",
	);
	let tracker = FakeTracker::new(vec![
		no_priority.clone(),
		newest_same_priority.clone(),
		oldest_same_priority.clone(),
		high_priority.clone(),
	]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![no_priority, newest_same_priority, oldest_same_priority, high_priority],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-102");
}

#[test]
fn candidate_selection_breaks_ties_by_identifier_after_created_at() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let later_identifier = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let earlier_identifier = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-101",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![later_identifier.clone(), earlier_identifier.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![later_identifier, earlier_identifier],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-101");
}

#[test]
fn candidate_selection_does_not_requery_queue_label_for_truncated_candidates() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()])
		.with_label_lookup_issues(&queue_label, vec![issue.clone()]);
	let mut truncated_issue = issue.clone();

	truncated_issue.labels_complete = false;

	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![truncated_issue],
		&workflow,
		&state_store,
		TEST_SERVICE_ID,
	)
	.expect("candidate selection should succeed")
	.expect("queue candidate should remain selectable");

	assert_eq!(selected.identifier, issue.identifier);
	assert!(tracker.label_queries.borrow().is_empty());
}

#[test]
fn candidate_selection_skips_todo_issue_with_nonterminal_blockers() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut blocked_high_priority = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:15:17.133Z",
	);

	blocked_high_priority.blockers =
		vec![tests::sample_blocker("issue-9", "PUB-109", "In Progress")];

	let unblocked_lower_priority = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-103",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker =
		FakeTracker::new(vec![blocked_high_priority.clone(), unblocked_lower_priority.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![blocked_high_priority, unblocked_lower_priority],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-103");
}

#[test]
fn candidate_selection_allows_dispatch_when_another_issue_has_active_lease() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_lease("pubfi", "issue-active", "run-1", "In Progress")
		.expect("lease should record");

	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![issue],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed");

	assert!(
		selected.is_some(),
		"another active lease must not impose a project-level dispatch cap"
	);
}

#[test]
fn candidate_selection_blocks_ordinary_dispatch_for_retained_review_handoff_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("Todo", &[]);
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

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![issue],
		&workflow,
		&state_store,
		config.service_id(),
	)
	.expect("candidate selection should succeed");

	assert!(
		selected.is_none(),
		"ordinary intake must not mint a duplicate attempt for a retained review handoff lane"
	);
}

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

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&repair_issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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
	let repair_issue = candidate_selection_service_owned_issue("In Review");
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

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&repair_issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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
	let repair_issue = candidate_selection_service_owned_issue("In Review");
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
fn targeted_retry_blocks_retained_review_handoff_marker_in_state_transition_window() {
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

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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

#[test]
fn plan_project_issue_run_prefers_post_review_closeout_lane_over_normal_candidate() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let normal_issue = tests::sample_issue("Todo", &[]);
	let closeout_issue = candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![normal_issue.clone(), closeout_issue.clone()],
		vec![
			vec![closeout_issue.clone()],
			vec![closeout_issue.clone()],
			vec![closeout_issue.clone()],
		],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&head_oid,
			"waiting_for_merge",
			1,
		),
	);

	let _path_guard =
		tests::install_fake_merged_pr_gh_response(&temp_dir, &worktree, pr_url, &head_oid);
	let mut merged_review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	let inspector = FakePullRequestReviewStateInspector::new(vec![
		Ok(merged_review_state.clone()),
		Ok(merged_review_state),
	]);
	let selected = orchestrator::select_post_review_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review closeout selection should succeed")
	.expect("closeout lane should be selected");

	assert_eq!(selected.issue.identifier, closeout_issue.identifier);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);
	assert_eq!(
		selected
			.preferred_run_identity
			.as_ref()
			.map(|identity| (identity.run_id.as_str(), identity.attempt_number)),
		Some(("run-1", 1))
	);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![closeout_issue.clone()],
		vec![vec![closeout_issue.clone()], vec![closeout_issue.clone()]],
	);
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &closeout_issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("targeted closeout planning should succeed")
	.expect("closeout issue run should plan");

	assert_eq!(summary.issue_identifier, closeout_issue.identifier);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);
	assert_eq!(summary.issue_state, "In Review");
}

#[test]
fn plan_project_issue_run_allows_merged_closeout_after_retry_budget() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(&base_config, "HOME");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let closeout_issue = candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![closeout_issue.clone()],
		vec![
			vec![closeout_issue.clone()],
			vec![closeout_issue.clone()],
			vec![closeout_issue.clone()],
		],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&head_oid,
			"waiting_for_merge",
			1,
		),
	);

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-closeout-{attempt}"),
				&closeout_issue.id,
				attempt,
				"failed",
			)
			.expect("failed attempt should record");
	}

	let _path_guard =
		tests::install_fake_merged_pr_gh_response(&temp_dir, &worktree, pr_url, &head_oid);
	let mut merged_review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	let inspector = FakePullRequestReviewStateInspector::new(vec![
		Ok(merged_review_state.clone()),
		Ok(merged_review_state),
	]);
	let selected = orchestrator::select_post_review_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review closeout selection should succeed")
	.expect("closeout lane should be selected");

	assert_eq!(selected.issue.identifier, closeout_issue.identifier);
	assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![closeout_issue.clone()],
		vec![vec![closeout_issue.clone()], vec![closeout_issue.clone()]],
	);
	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &closeout_issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: true,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("targeted closeout planning should succeed")
	.expect("closeout issue run should plan");

	assert_eq!(summary.issue_identifier, closeout_issue.identifier);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Closeout);
}

#[test]
fn retained_closeout_identity_reuse_respects_attempt_history() {
	{
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = candidate_selection_service_owned_issue("In Review");
		let identity = RetainedReviewRunIdentity {
			run_id: String::from("pub-101-attempt-1-111"),
			attempt_number: 1,
		};

		assert!(
			orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("missing attempts should be reusable for recovered closeout")
		);

		state_store
			.record_run_attempt(&identity.run_id, &issue.id, identity.attempt_number, "failed")
			.expect("failed attempt should record");

		assert!(
			!orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("failed attempts should not be reused for closeout")
		);
		assert_eq!(
			state_store.next_attempt_number(&issue.id).expect("next attempt should calculate"),
			2,
			"actual failed closeout attempts should still allocate the next attempt"
		);
	}
	{
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = candidate_selection_service_owned_issue("In Review");
		let identity = RetainedReviewRunIdentity {
			run_id: String::from("pub-101-attempt-1-111"),
			attempt_number: 1,
		};

		state_store
			.record_run_attempt(&identity.run_id, &issue.id, identity.attempt_number, "succeeded")
			.expect("completed handoff attempt should record");
		state_store
			.record_run_attempt("pub-101-attempt-2-222", &issue.id, 2, "succeeded")
			.expect("later non-retry attempt should record");

		assert!(
			orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("later non-retry attempts should not block handoff identity reuse")
		);
		assert_eq!(
			state_store.next_attempt_number(&issue.id).expect("next attempt should calculate"),
			3,
			"non-retry local history may still know about later attempts"
		);
	}

	for status in ["failed", "interrupted", TERMINAL_GUARDED_RUN_STATUS] {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = candidate_selection_service_owned_issue("In Review");
		let identity = RetainedReviewRunIdentity {
			run_id: String::from("pub-101-attempt-1-111"),
			attempt_number: 1,
		};
		let retry_run_id = format!("pub-101-attempt-2-{status}");

		state_store
			.record_run_attempt(&identity.run_id, &issue.id, identity.attempt_number, "succeeded")
			.expect("completed handoff attempt should record");
		state_store
			.record_run_attempt(&retry_run_id, &issue.id, 2, status)
			.expect("later closeout retry should record");

		assert!(
			!orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("later retry-budget attempts should block handoff identity reuse"),
			"later `{status}` closeout retry should block handoff identity reuse"
		);
		assert_eq!(
			state_store.next_attempt_number(&issue.id).expect("next attempt should calculate"),
			3,
			"real `{status}` closeout retries should keep incrementing"
		);
	}
}

#[test]
fn non_github_review_retained_drain_handles_same_issue_closeout_after_merge_visibility() {
	for closeout_available in [true, false] {
		let (temp_dir, config, workflow) = tests::temp_project_layout();
		let repo_root = config.repo_root().to_path_buf();
		let pr_url = "https://github.com/hack-ink/decodex/pull/176";
		let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
		let landed_merge_subject = r#"{"schema":"decodex/commit/1","summary":"Land current retained handoff","authority":"PUB-101"}"#;
		let head_oid =
			tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
		let (gh_command_path, invocation_log_path) =
			tests::install_fake_admin_merge_gh_response_with_merge_exit_code(
				&temp_dir, &head_oid, 0,
			);
		let config = tests::service_config_with_review_level(
			&tests::service_config_with_github_token_env_var_and_command_path(
				&config,
				"PATH",
				&gh_command_path,
			),
			ReviewLevel::Standard,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = candidate_selection_service_owned_issue("In Review");
		let tracker = FakeTracker::with_refresh_snapshots(
			vec![issue.clone()],
			vec![
				vec![issue.clone()],
				vec![issue.clone()],
				vec![issue.clone()],
				vec![issue.clone()],
			],
		);

		state_store
			.upsert_worktree(
				config.service_id(),
				&issue.id,
				"main",
				&repo_root.display().to_string(),
			)
			.expect("worktree should record");

		tests::seed_review_handoff_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
			&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
		);

		let open_review_state = tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);
		let mut merged_review_state = open_review_state.clone();

		merged_review_state.state = String::from("MERGED");

		let handoff_summary = sample_handoff_summary(&issue, &repo_root);
		let closeout_summary = RunSummary {
			dispatch_mode: IssueDispatchMode::Closeout,
			issue_state: String::from("In Review"),
			initial_issue_state: String::from("In Review"),
			..handoff_summary.clone()
		};
		let closeout_dispatches = RefCell::new(Vec::new());
		let drained = orchestrator::drain_non_github_review_retained_tail_with_inspector(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&handoff_summary,
			&FakePullRequestReviewStateInspector::new(vec![
				Ok(open_review_state.clone()),
				Ok(open_review_state),
				Ok(merged_review_state.clone()),
				Ok(merged_review_state),
			]),
			|source_summary| {
				closeout_dispatches.borrow_mut().push(source_summary.issue_id.clone());

				assert_eq!(source_summary.run_id, handoff_summary.run_id);
				assert_eq!(source_summary.attempt_number, handoff_summary.attempt_number);

				if closeout_available { Ok(Some(closeout_summary.clone())) } else { Ok(None) }
			},
		)
		.expect("non-GitHub-review retained drain should succeed");

		assert_eq!(drained, closeout_available.then_some(closeout_summary.clone()));
		assert_eq!(*closeout_dispatches.borrow(), vec![issue.id.clone()]);

		let marker = tests::persisted_review_orchestration_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
		);

		assert_eq!(marker.phase(), "waiting_for_merge");

		assert_admin_merge_invocation(
			&invocation_log_path,
			&head_oid,
			landed_merge_subject,
			pr_url,
		);
	}
}

fn assert_admin_merge_invocation(
	invocation_log_path: &Path,
	head_oid: &str,
	landed_merge_subject: &str,
	pr_url: &str,
) {
	let gh_invocation = fs::read_to_string(invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(
		gh_invocation,
		vec![
			String::from("pr"),
			String::from("merge"),
			String::from("--admin"),
			String::from("--merge"),
			String::from("--match-head-commit"),
			String::from(head_oid),
			String::from("--subject"),
			String::from(landed_merge_subject),
			String::from("--body"),
			String::new(),
			String::from(pr_url),
			String::from("pr"),
			String::from("view"),
			String::from(pr_url),
			String::from("--json"),
			String::from("state,headRefOid,mergeCommit"),
		]
	);
}

#[test]
fn non_github_review_retained_drain_stops_cleanly_when_checks_are_pending() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_review_level(&config, ReviewLevel::Standard);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let repo_root = config.repo_root().to_path_buf();
	let head_oid = tests::git_output(&repo_root, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/177";

	state_store
		.upsert_worktree(config.service_id(), &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
	);

	let handoff_summary = sample_handoff_summary(&issue, &repo_root);
	let closeout_dispatches = RefCell::new(Vec::new());
	let drained = orchestrator::drain_non_github_review_retained_tail_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&handoff_summary,
		&FakePullRequestReviewStateInspector::new(vec![
			Ok(tests::sample_pull_request_review_state(
				pr_url,
				"main",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("PENDING"),
				0,
			)),
			Ok(tests::sample_pull_request_review_state(
				pr_url,
				"main",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("PENDING"),
				0,
			)),
		]),
		|source_summary| {
			closeout_dispatches.borrow_mut().push(source_summary.issue_id.clone());

			Ok(None)
		},
	)
	.expect("pending checks should stop the retained drain cleanly");

	assert!(drained.is_none());
	assert!(closeout_dispatches.borrow().is_empty());

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "request_pending");
}

#[test]
fn post_review_closeout_selection_skips_completed_issue_with_open_pull_request() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let closeout_issue = candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![closeout_issue.clone()],
		vec![vec![closeout_issue.clone()]],
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/175";

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let open_pr_review_state = tests::sample_pull_request_review_state(
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
		Ok(open_pr_review_state.clone()),
		Ok(open_pr_review_state),
	]);
	let selected = orchestrator::select_post_review_closeout_issue_candidate_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&[],
		&inspector,
	)
	.expect("post-review closeout selection should succeed");

	assert!(
		selected.is_none(),
		"completed issues should not auto-dispatch closeout until the PR is merged"
	);
}

#[test]
fn closeout_dispatch_policy_rejects_open_pull_request() {
	for state_name in ["Done", "In Review"] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let closeout_issue = candidate_selection_service_owned_issue(state_name);
		let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let worktree_manager =
			WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
		let worktree = worktree_manager
			.ensure_worktree(&closeout_issue.identifier, false)
			.expect("worktree should exist");
		let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
		let pr_url = "https://github.com/hack-ink/decodex/pull/176";

		tests::seed_review_handoff_marker(
			&state_store,
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			pr_url,
			&head_oid,
		);

		let open_pr_review_state = tests::sample_pull_request_review_state(
			pr_url,
			&worktree.branch_name,
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);
		let dispatch_inspector =
			FakePullRequestReviewStateInspector::new(vec![Ok(open_pr_review_state.clone())]);
		let block_reason_inspector =
			FakePullRequestReviewStateInspector::new(vec![Ok(open_pr_review_state)]);

		assert!(
			!orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
				&tracker,
				&closeout_issue,
				&config,
				&workflow,
				&state_store,
				&dispatch_inspector,
			)
			.expect("dispatch policy inspection should succeed"),
			"{state_name} closeout issues must wait until the retained PR is merged",
		);
		assert_eq!(
			orchestrator::closeout_dispatch_block_reason_with_inspector(
				&tracker,
				&closeout_issue,
				&config,
				&workflow,
				&state_store,
				&block_reason_inspector,
			)
			.expect("block reason inspection should succeed"),
			Some("pull_request_not_merged"),
			"{state_name} closeout issues with open PRs should stay blocked, not ineligible",
		);
	}
}

#[test]
fn closeout_dispatch_policy_allows_completed_issue_after_pull_request_merges() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/177";

	tests::seed_review_handoff_marker(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
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
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&inspector,
		)
		.expect("dispatch policy inspection should succeed"),
		"completed issues should pass closeout dispatch after the retained PR merges",
	);
}

#[test]
fn closeout_dispatch_policy_blocks_completed_issue_with_missing_review_handoff_record() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");

	state_store
		.upsert_worktree(
			config.service_id(),
			&closeout_issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("retained closeout worktree mapping should persist");

	assert!(
		!orchestrator::issue_passes_closeout_dispatch_policy(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
		)
		.expect("dispatch policy inspection should succeed"),
		"completed issues with missing review handoff must remain non-dispatchable",
	);
	assert_eq!(
		orchestrator::closeout_dispatch_block_reason(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
		)
		.expect("block reason inspection should succeed"),
		Some("missing_review_handoff_record"),
		"completed issues with retained worktrees but no review handoff should stay retained as blocked lanes",
	);
}

#[test]
fn closeout_dispatch_policy_rejects_completed_issue_without_service_active_label() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = tests::sample_issue("Done", &[]);
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/177";

	tests::seed_review_handoff_marker(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
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
		!orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&inspector,
		)
		.expect("dispatch policy inspection should succeed"),
		"completed issues without service ownership must not pass closeout dispatch",
	);
	assert_eq!(
		orchestrator::closeout_dispatch_block_reason_with_inspector(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(Vec::new()),
		)
		.expect("block reason inspection should succeed"),
		None,
		"ownership-gated closeout issues should become ineligible rather than retained as blocked lanes",
	);
}

#[test]
fn closeout_dispatch_policy_uses_matching_handoff_record_for_current_branch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let closeout_issue = candidate_selection_service_owned_issue("Done");
	let tracker = FakeTracker::new(vec![closeout_issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&closeout_issue.identifier, false)
		.expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let current_pr_url = "https://github.com/hack-ink/decodex/pull/177";

	tests::seed_review_handoff_marker(
		&state_store,
		config.service_id(),
		&closeout_issue.id,
		&worktree.branch_name,
		current_pr_url,
		&head_oid,
	);

	state_store
		.upsert_review_handoff_marker(
			config.service_id(),
			&closeout_issue.id,
			&ReviewHandoffMarker::new(
				String::from("run-review-handoff-newer"),
				2,
				String::from("x/pubfi-pub-101-next"),
				String::from("https://github.com/hack-ink/decodex/pull/999"),
				String::from("release/9.x"),
				String::from("x/pubfi-pub-101-next"),
				String::from("feedface"),
			),
		)
		.expect("unrelated branch handoff should persist");

	let mut merged_review_state = tests::sample_pull_request_review_state(
		current_pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	merged_review_state.state = String::from("MERGED");

	assert!(
		orchestrator::issue_passes_closeout_dispatch_policy_with_inspector(
			&tracker,
			&closeout_issue,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(vec![Ok(merged_review_state)]),
		)
		.expect("dispatch policy inspection should succeed"),
		"matching branch handoff records should remain dispatchable even when newer tracker comments belong to another branch",
	);
}

#[test]
fn non_dry_run_closeout_dispatch_errors_when_pr_state_read_fails() {
	let (_temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_github_token_env_var(
		&base_config,
		"DECODEX_TEST_MISSING_DIRECT_DELIVERY_CLOSEOUT_GITHUB_TOKEN",
	);
	let issue = candidate_selection_service_owned_issue("Done");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/179";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let error = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &tracker,
		project: &config,
		workflow: &workflow,
		state_store: &state_store,
		issue_id: &issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: Some("In Review"),
		dry_run: false,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect_err("non-dry-run closeout dispatch should surface GH state read failures");

	assert!(error.to_string().contains("pull_request_state_read_failed"));
}

#[test]
fn candidate_selection_skips_issue_claimed_by_another_process() {
	let workflow = WorkflowDocument::parse_markdown(&tests::sample_workflow_markdown(
		"pubfi",
		&[],
		"Claim-aware workflow policy.\n",
		1,
	))
	.expect("workflow should parse");
	let (_temp_dir, config, _default_workflow) = tests::temp_project_layout();
	let remote_store = StateStore::open_in_memory().expect("remote state store should open");
	let local_store = StateStore::open_in_memory().expect("local state store should open");
	let claimed_issue = tests::sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-100",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let free_issue = tests::sample_issue_with_sort_fields(
		"issue-free",
		"PUB-101",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:18.133Z",
	);

	remote_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("remote dispatch-slot root should configure");
	local_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("local dispatch-slot root should configure");

	assert!(
		remote_store
			.try_acquire_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
			.expect("remote issue claim should succeed")
	);

	let tracker = FakeTracker::new(vec![claimed_issue.clone(), free_issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![claimed_issue, free_issue.clone()],
		&workflow,
		&local_store,
		config.service_id(),
	)
	.expect("candidate selection should succeed")
	.expect("the unclaimed issue should still be selected");

	assert_eq!(selected.id, free_issue.id);
}
