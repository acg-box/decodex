use std::fs;

use crate::{
	orchestrator::{
		self, StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, FakeTracker, review_landing_status_support},
	},
	test_support,
	worktree::WorktreeManager,
};

#[test]
fn build_post_review_lane_statuses_leaves_managed_worktree_git_metadata_untouched() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["config", "--local", "codex.github-identity", "y"])
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["config", "--local", "codex.linear-workspace", "hackink"])
			.status()
			.expect("git config should run")
			.success()
	);

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

	review_landing_status_support::remove_local_git_metadata_for_post_review_status(&worktree.path);

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
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
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

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "ready_to_land");
	assert_eq!(tests::try_git_local_config_value(&worktree.path, "codex.github-identity"), None);
	assert_eq!(tests::try_git_local_config_value(&worktree.path, "codex.linear-workspace"), None);
	assert_eq!(tests::git_remote_url(&worktree.path, "origin"), None);
}

#[test]
fn build_post_review_lane_statuses_blocks_missing_review_handoff_record() {
	for managed_worktree in [false, true] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let issue = tests::sample_issue("In Review", &[]);
		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let state_store = StateStore::open_in_memory().expect("state store should open");

		if managed_worktree {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);
			let worktree = worktree_manager
				.ensure_worktree(&issue.identifier, false)
				.expect("worktree should exist");

			state_store
				.upsert_worktree(
					config.service_id(),
					&issue.id,
					&worktree.branch_name,
					&worktree.path.display().to_string(),
				)
				.expect("worktree should record");
		} else {
			let repo_root = config.repo_root().to_path_buf();

			state_store
				.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
				.expect("worktree should record");
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
		assert_eq!(lanes[0].reason, "missing_review_handoff_record");
	}
}

#[test]
fn build_post_review_lane_statuses_allows_descendant_review_handoff_head_after_repair_push() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let marker_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let current_head_oid =
		tests::commit_worktree_change(&worktree.path, "repair.txt", "repair push\n", "repair push");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

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
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &marker_head_oid),
	);
	tests::seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&tests::sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&current_head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&current_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "ready_to_land");
	assert_eq!(lanes[0].reason, "external_review_passed_strict");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}

#[test]
fn build_post_review_lane_statuses_blocks_review_handoff_lineage_rewrite() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let issue = tests::sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let marker_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	tests::git_status_success(&worktree.path, &["checkout", "--orphan", "rewrite-history"]);
	fs::write(worktree.path.join("rewrite.txt"), "rewritten history\n")
		.expect("rewrite file should write");
	tests::git_status_success(&worktree.path, &["add", "rewrite.txt"]);
	tests::git_status_success(&worktree.path, &["commit", "-m", "rewrite history"]);
	tests::git_status_success(&worktree.path, &["branch", "-M", &worktree.branch_name]);

	let rewritten_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);

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
		&tests::sample_review_handoff_marker(&worktree.branch_name, pr_url, &marker_head_oid),
	);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				pr_url,
				&worktree.branch_name,
				&rewritten_head_oid,
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
	assert_eq!(lanes[0].reason, "review_handoff_lineage_mismatch");
	assert_eq!(lanes[0].readback_root_cause.as_deref(), Some("lineage_validation_failed"));
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}
