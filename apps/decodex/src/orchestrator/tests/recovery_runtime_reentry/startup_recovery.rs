use std::{fs, process};

use crate::{
	orchestrator::{
		self, ReviewLevel,
		tests::{self, FakeTracker, TEST_SERVICE_ID, recovery_terminal_support},
	},
	state::{self, ReviewPolicyCheckpointInput, StateStore},
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn materialize_run_summary_worktree_creates_worktree_before_child_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("dry-run planning should succeed")
		.expect("brand-new lane should be selected");

	assert!(
		!summary.worktree_path.exists(),
		"dry-run planning should not materialize the worktree yet"
	);

	let worktree = orchestrator::materialize_run_summary_worktree(&config, &workflow, &summary)
		.expect("daemon parent should materialize the worktree before child startup");

	assert_eq!(worktree.path, summary.worktree_path);
	assert_eq!(worktree.branch_name, summary.branch_name);
	assert!(
		worktree.path.exists(),
		"materialized worktree should exist before writing child activity markers"
	);

	state::write_run_activity_marker_for_process(
		&worktree.path,
		&summary.run_id,
		summary.attempt_number,
		process::id(),
	)
	.expect("child activity marker should write after worktree materialization");
}

#[test]
fn recover_runtime_state_recovers_fresh_review_repair_activity_marker() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Review");
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("review-repair worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-review-repair", 1)
		.expect("fresh activity marker should write");

	let recovered = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
		&tracker,
		&config,
		&workflow,
		&state_store,
	)
	.expect("runtime recovery should succeed");

	assert!(
		recovered.recoverable_issues.is_empty(),
		"fresh review-repair activity should rebuild the lease instead of requeueing the lane"
	);

	let lease = state_store
		.lease_for_issue(&issue.id)
		.expect("lease lookup should succeed")
		.expect("fresh review-repair lane should rebuild its lease");

	assert_eq!(lease.run_id(), "run-review-repair");
	assert_eq!(lease.issue_state(), workflow.frontmatter().tracker().success_state());
}

#[test]
fn prefers_recovered_in_progress_worktree() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = recovery_terminal_support::sample_active_issue("In Progress");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovered dry run should succeed")
		.expect("active recovered issue should be selected");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping should be reconstructed from the retained lane"
	);
}

#[test]
fn run_project_once_recovers_ready_post_review_lane_before_landing() {
	let (temp_dir, base_config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_review_level(
		&tests::service_config_with_github_token_env_var(&base_config, "PATH"),
		ReviewLevel::Standard,
	);
	let issue = recovery_terminal_support::sample_active_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()], vec![issue.clone()]],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained review worktree should be created");
	let pr_url = "https://github.com/hack-ink/decodex/pull/333";
	let head_subject = r#"{"schema":"decodex/commit/2","change":"Add retry hint","authority":"PUB-101","impact":"compatible"}"#;
	let landed_subject = r#"{"schema":"decodex/commit/2","change":"Land Add retry hint","authority":"PUB-101","impact":"compatible"}"#;
	let head_oid = tests::commit_worktree_change(
		&worktree.path,
		"retained-ready.txt",
		"ready\n",
		head_subject,
	);
	let (_path_guard, invocation_log_path) =
		recovery_terminal_support::install_fake_ready_to_land_admin_merge_gh_response(
			&temp_dir, &worktree, pr_url, &head_oid,
		);

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: config.service_id(),
			issue_id: &issue.id,
			run_id: "runtime-review",
			attempt_number: 1,
			phase: "handoff",
			review_level: "standard",
			status: "clean",
			head_sha: &head_oid,
			nonclean_rounds: 0,
			details_json: "{}",
		})
		.expect("runtime clean review checkpoint should seed");

	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, false)
		.expect("recovered retained post-review lane should reconcile");

	assert!(
		summary.is_none(),
		"ready retained post-review landing should not dispatch a new current lane"
	);

	let marker = tests::persisted_review_lifecycle_transition_fixture_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
	);
	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(marker.phase(), "landed");
	assert_eq!(
		gh_invocation,
		vec![
			String::from("pr"),
			String::from("merge"),
			String::from("--admin"),
			String::from("--merge"),
			String::from("--match-head-commit"),
			head_oid,
			String::from("--subject"),
			String::from(landed_subject),
			String::from("--body"),
			String::new(),
			String::from(pr_url),
		]
	);
}

#[test]
fn run_project_once_recovers_retained_worktree_from_issue_identifier() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue_with_project_slug_and_sort_fields(
		"issue-1",
		"PUB-101",
		"tracker-project",
		"In Progress",
		&[active_label.as_str()],
		Some(3),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let expected_worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("recovered worktree should be created")
		.path;
	let summary = orchestrator::run_project_once(&tracker, &config, &workflow, &state_store, true)
		.expect("recovered dry run should succeed")
		.expect("active recovered issue should be selected");

	assert_eq!(summary.issue_id, issue.id);
	assert_eq!(summary.dispatch_mode, orchestrator::IssueDispatchMode::Retry);
	assert_eq!(summary.worktree_path, expected_worktree);
}
