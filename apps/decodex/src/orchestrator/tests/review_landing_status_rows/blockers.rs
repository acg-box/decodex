use std::fs;

use crate::{
	orchestrator::{
		self, ReviewHandoffMarker, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker},
	},
	test_support,
	worktree::WorktreeManager,
};

#[test]
fn build_post_review_lane_statuses_blocks_not_dispatchable_labeled_post_review_issues() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	for (labels, expected_reason) in [
		(&["decodex:manual-only"][..], "issue_opted_out"),
		(&["decodex:needs-attention"][..], "issue_needs_attention"),
	] {
		let issue = tests::sample_issue("In Review", labels);

		state_store
			.upsert_worktree(
				config.service_id(),
				&issue.id,
				"x/pubfi-pub-101",
				&config.repo_root().display().to_string(),
			)
			.expect("worktree should record");

		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let lanes = orchestrator::build_post_review_lane_statuses(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(Vec::new()),
		)
		.expect("post-review lane status build should succeed");

		assert_eq!(lanes.len(), 1);
		assert_eq!(lanes[0].classification, "blocked");
		assert_eq!(lanes[0].reason, expected_reason);

		state_store.clear_worktree(&issue.id).expect("worktree should clear between label cases");
	}
}

#[test]
fn build_post_review_lane_statuses_blocks_exhausted_retry_budget() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-101",
			&config.repo_root().display().to_string(),
		)
		.expect("worktree should record");

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(&format!("run-{attempt}"), &issue.id, attempt, "failed")
			.expect("failed attempt should record");
	}

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "retry_budget_exhausted");
}

#[test]
fn build_post_review_lane_statuses_keeps_unmerged_retry_budget_blocked() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/120";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewHandoffMarker::new(
			"run-review-handoff",
			1,
			&worktree.branch_name,
			pr_url,
			"main",
			&worktree.branch_name,
			&head_oid,
		),
	);

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
			.record_run_attempt(&format!("run-{attempt}"), &issue.id, attempt, "failed")
			.expect("failed attempt should record");
	}

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				pr_url,
				&worktree.branch_name,
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "retry_budget_exhausted");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_state.as_deref(), Some("OPEN"));
}

#[test]
fn build_post_review_lane_statuses_blocks_worktree_head_read_failures() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let branch_ref_path =
		config.repo_root().join(".git").join("refs").join("heads").join(&worktree.branch_name);
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&worktree.path)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	tests::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	fs::remove_file(&branch_ref_path).expect("branch ref should remove");

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "worktree_head_read_failed");
}

#[test]
fn build_post_review_lane_statuses_blocks_missing_worktree_checkout_branch() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = String::from_utf8(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&worktree.path)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(&worktree.path)
			.args(["checkout", "--detach", &head_oid])
			.status()
			.expect("git checkout --detach should run")
			.success()
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "worktree_checkout_branch_missing");
}
