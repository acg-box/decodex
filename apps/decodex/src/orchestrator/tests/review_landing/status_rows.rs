#[test]
fn build_post_review_lane_statuses_reports_ready_to_land() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		Command::new("git")
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_orchestration_marker("main", pr_url, &head_oid, "waiting_for_result", 1),
	);

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	add_external_review_ack(&mut review_state);
	add_external_review_pass(&mut review_state);

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
fn build_post_review_lane_statuses_preserves_handoff_marker_when_pr_readback_fails() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		Command::new("git")
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &head_oid),
	);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Err(color_eyre::eyre::eyre!(
			"gh api failed"
		))]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].issue_identifier, issue.identifier);
	assert_eq!(lanes[0].classification, "wait_for_review");
	assert_eq!(lanes[0].reason, "pull_request_state_read_failed");
	assert_eq!(lanes[0].branch_name, "main");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_head_sha.as_deref(), Some(head_oid.as_str()));
	assert_eq!(
		lanes[0].readback_warning.as_deref(),
		Some("pull_request_state_read_failed")
	);
	assert_eq!(lanes[0].pr_state, None);
}

#[test]
fn build_post_review_lane_statuses_skips_external_review_when_disabled() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_external_review_enabled(&config, false);
	let repo_root = config.repo_root().to_path_buf();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		Command::new("git")
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &head_oid),
	);

	let review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);
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
	assert_eq!(lanes[0].reason, "internal_review_only_ready_to_land");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}

#[test]
fn build_post_review_lane_statuses_routes_mixed_external_pass_and_feedback_to_repair() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		Command::new("git")
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_orchestration_marker("main", pr_url, &head_oid, "waiting_for_result", 1),
	);

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	add_external_review_ack(&mut review_state);

	review_state.issue_description_external_review_thumbs_up_count = 1;

	add_external_review_summary(
		&mut review_state,
		"Didn't find any major issues. Please fix X.",
		"COMMENTED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
	);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "needs_review_repair");
	assert_eq!(lanes[0].reason, "external_review_feedback_pending_repair");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}

#[test]
fn build_post_review_lane_statuses_ignores_non_external_review_signals() {
	for (phase, signal, expected_reason) in [
		("waiting_for_ack", "ack", "external_review_ack_pending"),
		("waiting_for_result", "pass", "external_review_result_pending"),
	] {
		let (_temp_dir, config, workflow) = temp_project_layout();
		let repo_root = config.repo_root().to_path_buf();
		let issue = sample_issue("In Review", &[]);
		let tracker =
			FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let head_oid = String::from_utf8(
			Command::new("git")
				.arg("-C")
				.arg(&repo_root)
				.args(["rev-parse", "HEAD"])
				.output()
				.expect("git rev-parse should run")
				.stdout,
		)
		.expect("git output should be utf-8")
		.trim()
		.to_owned();
		let pr_url = "https://github.com/hack-ink/decodex/pull/173";

		state_store
			.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
			.expect("worktree should record");

		seed_review_handoff_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
			&sample_review_handoff_marker("main", pr_url, &head_oid),
		);
		seed_review_orchestration_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
			&sample_review_orchestration_marker("main", pr_url, &head_oid, phase, 1),
		);

		let mut review_state = sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);

		match signal {
			"ack" => add_review_request_ack_from_actor(
				&mut review_state,
				TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN,
			),
			"pass" => {
				add_external_review_ack(&mut review_state);
				add_external_review_pass_from_actor(
					&mut review_state,
					TEST_NON_EXTERNAL_REVIEW_ACTOR_LOGIN,
				);
			}

			_ => unreachable!("test case should use a known non-external signal"),
		}

		let lanes = orchestrator::build_post_review_lane_statuses(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		)
		.expect("post-review lane status build should succeed");

		assert_eq!(lanes.len(), 1);
		assert_eq!(lanes[0].classification, "wait_for_review");
		assert_eq!(lanes[0].reason, expected_reason);
	}
}

#[test]
fn build_post_review_lane_statuses_accepts_existing_description_thumbs_up_for_later_pass_rounds() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		Command::new("git")
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &head_oid),
	);
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"main",
			pr_url,
			&head_oid,
			"waiting_for_result",
			Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
			Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
			Some(1),
			0,
			1,
			None,
		),
	);

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	add_external_review_ack(&mut review_state);

	review_state.issue_description_external_review_thumbs_up_count = 1;

	add_external_review_summary(
		&mut review_state,
		EXTERNAL_REVIEW_PASS_PHRASE,
		"APPROVED",
		TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT + 1,
	);

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
fn build_post_review_lane_statuses_keeps_completed_issue_visible_for_closeout_tail_work() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = sample_issue("Done", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		Command::new("git")
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &head_oid),
	);

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].issue_state, "Done");
	assert_eq!(lanes[0].classification, "continue");
	assert_eq!(lanes[0].reason, "pull_request_merged_closeout_pending");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}

#[test]
fn build_post_review_lane_statuses_keeps_merged_closeout_visible_after_retry_budget() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let head_oid = String::from_utf8(
		Command::new("git")
			.arg("-C")
			.arg(&repo_root)
			.args(["rev-parse", "HEAD"])
			.output()
			.expect("git rev-parse should run")
			.stdout,
	)
	.expect("git output should be utf-8")
	.trim()
	.to_owned();
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &head_oid),
	);

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(&format!("run-{attempt}"), &issue.id, attempt, "failed")
			.expect("failed attempt should record");
	}

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].issue_state, "In Review");
	assert_eq!(lanes[0].classification, "continue");
	assert_eq!(lanes[0].reason, "pull_request_merged_closeout_pending");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_state.as_deref(), Some("MERGED"));
}

#[test]
fn build_post_review_lane_statuses_keeps_merged_closeout_visible_after_landed_main_advances() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let pr_head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let merge_commit_oid =
		commit_worktree_change(&worktree.path, "landed.txt", "landed\n", "land retained lane");
	let current_head_oid =
		commit_worktree_change(&worktree.path, "later.txt", "later\n", "advance main later");
	let pr_url = "https://github.com/hack-ink/decodex/pull/203";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &pr_head_oid),
	);

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&pr_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");
	review_state.merge_commit_oid = Some(merge_commit_oid);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(git_output(&worktree.path, &["rev-parse", "HEAD"]), current_head_oid);
	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "continue");
	assert_eq!(lanes[0].reason, "pull_request_merged_closeout_pending");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_state.as_deref(), Some("MERGED"));
}

#[test]
fn build_post_review_lane_statuses_leaves_managed_worktree_git_metadata_untouched() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());

	assert!(
		Command::new("git")
			.arg("-C")
			.arg(config.repo_root())
			.args(["config", "--local", "codex.github-identity", "y"])
			.status()
			.expect("git config should run")
			.success()
	);
	assert!(
		Command::new("git")
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
		Command::new("git")
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

	remove_local_git_metadata_for_post_review_status(&worktree.path);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	add_external_review_ack(&mut review_state);
	add_external_review_pass(&mut review_state);

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
	assert_eq!(try_git_local_config_value(&worktree.path, "codex.github-identity"), None);
	assert_eq!(try_git_local_config_value(&worktree.path, "codex.linear-workspace"), None);
	assert_eq!(git_remote_url(&worktree.path, "origin"), None);
}

#[test]
fn build_post_review_lane_statuses_blocks_missing_review_handoff_record() {
	for managed_worktree in [false, true] {
		let (_temp_dir, config, workflow) = temp_project_layout();
		let issue = sample_issue("In Review", &[]);
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let marker_head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let current_head_oid =
		commit_worktree_change(&worktree.path, "repair.txt", "repair push\n", "repair push");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &marker_head_oid),
	);
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_orchestration_marker(
			&worktree.branch_name,
			pr_url,
			&current_head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = sample_pull_request_review_state(
		pr_url,
		&worktree.branch_name,
		&current_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	add_external_review_ack(&mut review_state);
	add_external_review_pass(&mut review_state);

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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let marker_head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";

	git_status_success(&worktree.path, &["checkout", "--orphan", "rewrite-history"]);

	fs::write(worktree.path.join("rewrite.txt"), "rewritten history\n")
		.expect("rewrite file should write");

	git_status_success(&worktree.path, &["add", "rewrite.txt"]);
	git_status_success(&worktree.path, &["commit", "-m", "rewrite history"]);
	git_status_success(&worktree.path, &["branch", "-M", &worktree.branch_name]);

	let rewritten_head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &marker_head_oid),
	);

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(sample_pull_request_review_state(
			pr_url,
			&worktree.branch_name,
			&rewritten_head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		))]),
	)
	.expect("post-review lane status build should succeed");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "blocked");
	assert_eq!(lanes[0].reason, "review_handoff_lineage_mismatch");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
}

#[test]
fn build_post_review_lane_statuses_blocks_nonactive_labeled_post_review_issues() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	for (labels, expected_reason) in [
		(&["decodex:manual-only"][..], "issue_opted_out"),
		(&["decodex:needs-attention"][..], "issue_needs_attention"),
	] {
		let issue = sample_issue("In Review", labels);

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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/120";

	seed_review_handoff_marker_value(
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
		&FakePullRequestReviewStateInspector::new(vec![Ok(sample_pull_request_review_state(
			pr_url,
			&worktree.branch_name,
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		))]),
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
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
		Command::new("git")
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

	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = sample_issue("In Review", &[]);
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = String::from_utf8(
		Command::new("git")
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
		Command::new("git")
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

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
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
