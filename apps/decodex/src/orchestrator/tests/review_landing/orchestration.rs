#[test]
fn reconcile_post_review_orchestration_requests_external_review_without_thumbs_up_baseline() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&config, "PATH");
	let _path_guard = install_fake_post_issue_comment_gh_response(
		&temp_dir,
		TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		"2025-11-03T00:00:00Z",
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let initial_review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(initial_review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_ack");
	assert_eq!(marker.request_description_thumbs_up_count(), None);
}

#[test]
fn reconcile_post_review_orchestration_uses_matching_handoff_record_for_current_branch() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&config, "PATH");
	let _path_guard = install_fake_post_issue_comment_gh_response(
		&temp_dir,
		TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		"2025-11-03T00:00:00Z",
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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
	let current_branch = "main";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			current_branch,
			&repo_root.display().to_string(),
		)
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker(current_branch, pr_url, &head_oid),
	);
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			current_branch,
			pr_url,
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let review_state = sample_pull_request_review_state(
		pr_url,
		current_branch,
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_ack");
	assert_eq!(marker.pr_url(), pr_url);
}

#[test]
fn reconcile_post_review_orchestration_rebinds_stale_head_marker_after_repair_push() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&config, "PATH");
	let _path_guard = install_fake_post_issue_comment_gh_response(
		&temp_dir,
		TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
		"2025-11-03T00:00:00Z",
	);
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let marker_head_oid = git_output(&repo_root, &["rev-parse", "HEAD"]);
	let current_head_oid =
		commit_worktree_change(&repo_root, "repair.txt", "repair push\n", "repair push");

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_handoff_marker("main", pr_url, &marker_head_oid),
	);
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&sample_review_orchestration_marker(
			"main",
			pr_url,
			&marker_head_oid,
			"waiting_for_result",
			1,
		),
	);

	let review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&current_head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should rebind stale marker without attention");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_ack");
	assert_eq!(marker.head_sha(), current_head_oid);
	assert_eq!(
		marker.request_comment_database_id(),
		Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID)
	);
	assert_eq!(marker.external_round_count(), 1);
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_skips_merged_landed_lineage_without_manual_attention() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let issue = post_review_sample_service_owned_issue("In Review");
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
	seed_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			&worktree.branch_name,
			pr_url,
			&current_head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
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

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("merged post-review orchestration should not fail");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&worktree.path,
	);

	assert_eq!(marker.phase(), "request_pending");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_runs_admin_merge_after_external_pass() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&config, "PATH");
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_response(&temp_dir);
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
	let landed_merge_subject = r#"{"schema":"decodex/commit/1","summary":"Land current retained handoff","authority":"PUB-101"}"#;
	let head_oid = commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);

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

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);
	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(marker.phase(), "waiting_for_merge");
	assert!(marker.auto_merge_enabled_at_unix_epoch().is_some());
	assert_eq!(
		gh_invocation,
		vec![
			String::from("pr"),
			String::from("merge"),
			String::from("--admin"),
			String::from("--merge"),
			String::from("--match-head-commit"),
			head_oid.clone(),
			String::from("--subject"),
			String::from(landed_merge_subject),
			String::from("--body"),
			String::new(),
			String::from(pr_url),
		]
	);
}

#[test]
fn reconcile_post_review_orchestration_routes_non_clean_landing_to_agent_fallback() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&config, "PATH");
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_response(&temp_dir);
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
	let head_oid = commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);

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
		"HAS_HOOKS",
		Some("SUCCESS"),
		0,
	);

	add_external_review_ack(&mut review_state);
	add_external_review_pass(&mut review_state);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "repair_required");
	assert!(
		!invocation_log_path.exists(),
		"non-clean retained landing must not invoke runtime admin merge"
	);
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_runs_admin_merge_without_external_review_when_disabled() {
	assert_reconcile_post_review_orchestration_runs_admin_merge_without_external_review(
		ReviewLevel::Standard,
	);
}

#[test]
fn reconcile_post_review_orchestration_runs_admin_merge_in_basic_review_level() {
	assert_reconcile_post_review_orchestration_runs_admin_merge_without_external_review(
		ReviewLevel::Basic,
	);
}

fn assert_reconcile_post_review_orchestration_runs_admin_merge_without_external_review(
	review_level: ReviewLevel,
) {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_review_level(
		&service_config_with_github_token_env_var(&config, "PATH"),
		review_level,
	);
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_response(&temp_dir);
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
	let landed_merge_subject = r#"{"schema":"decodex/commit/1","summary":"Land current retained handoff","authority":"PUB-101"}"#;
	let head_oid = commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);

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

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);
	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(marker.phase(), "waiting_for_merge");
	assert!(marker.auto_merge_enabled_at_unix_epoch().is_some());
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
			String::from(landed_merge_subject),
			String::from("--body"),
			String::new(),
			String::from(pr_url),
		]
	);
	assert!(
		tracker.comments.borrow().is_empty(),
		"runtime orchestration state should stay in StateStore rather than Linear comments",
	);
}

#[test]
fn reconcile_post_review_orchestration_routes_non_github_review_non_clean_landing_to_agent_fallback(
) {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_review_level(
		&service_config_with_github_token_env_var(&config, "PATH"),
		ReviewLevel::Standard,
	);
	let (_path_guard, invocation_log_path) = install_fake_admin_merge_gh_response(&temp_dir);
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
	let head_oid = commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);

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
		"HAS_HOOKS",
		Some("SUCCESS"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "repair_required");
	assert!(
		!invocation_log_path.exists(),
		"non-clean non-GitHub-review retained landing must not invoke runtime admin merge"
	);
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_tolerates_already_merged_merge_race() {
	let (temp_dir, config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&config, "PATH");
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let pr_url = "https://github.com/hack-ink/decodex/pull/173";
	let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
	let landed_merge_subject = r#"{"schema":"decodex/commit/1","summary":"Land current retained handoff","authority":"PUB-101"}"#;
	let head_oid = commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
	let (_path_guard, invocation_log_path) =
		install_fake_admin_merge_gh_response_with_merge_exit_code(&temp_dir, &head_oid, 1);

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

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should accept an already-merged PR race");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);
	let gh_invocation = fs::read_to_string(&invocation_log_path)
		.expect("fake gh invocation log should read")
		.lines()
		.map(str::to_owned)
		.collect::<Vec<_>>();

	assert_eq!(marker.phase(), "waiting_for_merge");
	assert!(marker.auto_merge_enabled_at_unix_epoch().is_some());
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
	assert!(
		tracker.comments.borrow().is_empty(),
		"already-merged race handling should persist orchestration in StateStore, not Linear comments",
	);
	assert!(tracker.label_additions.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_waits_for_green_checks_before_requesting_external_review() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("PENDING"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "request_pending");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_routes_fixable_ci_red_to_repair_before_requesting_external_review()
 {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"BLOCKED",
		Some("FAILURE"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "repair_required");

	let comments = tracker.comments.borrow();

	assert!(comments.is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_routes_thread_only_external_review_to_repair() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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
		1,
	);

	add_external_review_ack(&mut review_state);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "repair_required");

	let comments = tracker.comments.borrow();

	assert!(comments.is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_fails_closed_when_pull_request_is_closed() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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

	review_state.state = String::from("CLOSED");

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	assert_eq!(tracker.state_updates.borrow().len(), 1);
	assert_eq!(tracker.comments.borrow().len(), 1);
	assert_eq!(tracker.label_additions.borrow().len(), 1);
	assert_eq!(tracker.label_removals.borrow().len(), 1);
}

#[test]
fn reconcile_post_review_orchestration_skips_issue_with_active_lease() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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

	assert!(
		state_store
			.try_acquire_lease(
				config.service_id(),
				&issue.id,
				"active-repair-run",
				workflow.frontmatter().tracker().in_progress_state(),
			)
			.expect("active lease should acquire")
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should skip active repair lanes");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "waiting_for_result");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_fails_closed_when_review_handoff_is_missing() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_worktree("pubfi", &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should succeed");

	assert_eq!(tracker.state_updates.borrow().len(), 1);
	assert_eq!(tracker.comments.borrow().len(), 1);
	assert_eq!(tracker.label_additions.borrow().len(), 1);
	assert_eq!(tracker.label_removals.borrow().len(), 1);
}

#[test]
fn reconcile_post_review_orchestration_skips_issue_without_service_active_label() {
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
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(Vec::new()),
	)
	.expect("post-review orchestration should skip unowned retained lanes");

	let marker = persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "request_pending");
	assert!(tracker.comments.borrow().is_empty());
	assert!(tracker.state_updates.borrow().is_empty());
	assert!(tracker.label_updates.borrow().is_empty());
	assert!(tracker.label_additions.borrow().is_empty());
	assert!(tracker.label_removals.borrow().is_empty());
}

#[test]
fn reconcile_post_review_orchestration_blocks_unhandled_ci_red_before_requesting_external_review() {
	let (_temp_dir, config, workflow) = temp_project_layout();
	let repo_root = config.repo_root().to_path_buf();
	let issue = post_review_sample_service_owned_issue("In Review");
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
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let review_state = sample_pull_request_review_state(
		pr_url,
		"main",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"UNSTABLE",
		Some("FAILURE"),
		0,
	);

	orchestrator::reconcile_post_review_orchestration_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("post-review orchestration should succeed");

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("manual attention comment should be written");
	let ledger_event = records::parse_linear_execution_event_record(comment)
		.expect("manual attention comment should include an execution ledger event");

	assert_eq!(comments.len(), 1);
	assert_eq!(
		ledger_event.error_class.as_deref(),
		Some("external_review_request_ci_red_manual_attention")
	);
	assert_eq!(tracker.label_additions.borrow().len(), 1);
	assert_eq!(tracker.label_removals.borrow().len(), 1);
}
