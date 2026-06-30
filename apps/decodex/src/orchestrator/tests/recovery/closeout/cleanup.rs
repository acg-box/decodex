#[test]
fn cleanup_completed_post_review_lane_preserves_worktree_when_remote_delete_fails() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = sample_issue("Done", &[]);
	let _tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");
	let head_oid = git_output(config.repo_root(), &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/67";
	let worktree = WorktreeSpec {
		branch_name: String::from("main"),
		issue_identifier: issue.identifier.clone(),
		path: config.repo_root().to_path_buf(),
		reused_existing: true,
	};
	let _path_guard = install_fake_merged_pr_gh_response_with_delete_exit_code(
		&temp_dir,
		&worktree,
		pr_url,
		&head_oid,
		1,
	);

	initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&ReviewHandoffMarker::new(
			"run-closeout-cleanup",
			1,
			"main",
			pr_url,
			"main",
			"main",
			head_oid,
		),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"main",
			&config.repo_root().display().to_string(),
		)
		.expect("worktree should record");

	let issue_run = sample_closeout_issue_run(&issue, &worktree, "run-closeout-cleanup");
	let error = orchestrator::cleanup_completed_post_review_lane(
		&config,
		&workflow,
		&state_store,
		&issue_run,
	)
	.expect_err("remote branch delete failures must stop cleanup");

	assert!(
		error.to_string().contains("Failed to delete retained remote branch"),
		"cleanup should surface the remote delete failure"
	);
	assert!(
		config.repo_root().exists(),
		"cleanup must preserve the retained worktree when remote delete fails"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping must remain so cleanup can retry later"
	);
	assert!(
		!git_output(config.repo_root(), &["branch", "--list", "main"]).is_empty(),
		"cleanup must not mutate local branch state when remote delete fails before local cleanup"
	);
}

#[test]
fn merged_closeout_retry_exhaustion_reports_cleanup_blocker_with_pr_url_after_default_branch_dirty()
{
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = sample_issue("Done", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/119";
	let _path_guard = install_fake_merged_pr_gh_response(&temp_dir, &worktree, pr_url, &head_oid);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

	initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);

	assert!(
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);

	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	fs::write(config.repo_root().join("README.md"), "local repo override\n")
		.expect("tracked repo-root file should become dirty");

	let issue_run = sample_closeout_issue_run(&issue, &worktree, "run-closeout-dirty-root");
	let error = orchestrator::cleanup_completed_post_review_lane(
		&config,
		&workflow,
		&state_store,
		&issue_run,
	)
	.expect_err("tracked repo-root dirtiness must block default-branch sync");

	assert!(
		error.to_string().contains("tracked local changes"),
		"cleanup should surface the default-branch sync blocker: {error:?}"
	);
	assert!(worktree.path.exists(), "cleanup must preserve the retained worktree");

	for attempt in 1..=3 {
		state_store
			.record_run_attempt(
				&format!("run-closeout-dirty-root-{attempt}"),
				&issue.id,
				attempt,
				"failed",
			)
			.expect("failed closeout attempt should record");
	}

	let mut merged_review_state = sample_pull_request_review_state(
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

	let lanes = orchestrator::build_post_review_lane_statuses(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&FakePullRequestReviewStateInspector::new(vec![Ok(merged_review_state)]),
	)
	.expect("post-review status should classify the retained cleanup blocker");

	assert_eq!(lanes.len(), 1);
	assert_eq!(lanes[0].classification, "cleanup_blocked");
	assert_eq!(lanes[0].reason, "default_branch_worktree_dirty");
	assert_eq!(lanes[0].pr_url.as_deref(), Some(pr_url));
	assert_eq!(lanes[0].pr_state.as_deref(), Some("MERGED"));
}

#[test]
fn cleanup_completed_post_review_lane_fails_closed_when_pr_target_branch_drifted() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = sample_issue("Done", &[]);
	let _tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/180";
	let _path_guard = install_fake_merged_pr_gh_response_with_base_ref(
		&temp_dir,
		&worktree,
		pr_url,
		&head_oid,
		"release/1.x",
	);

	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: String::from("In Review"),
		worktree: WorktreeSpec {
			branch_name: worktree.branch_name.clone(),
			issue_identifier: issue.identifier.clone(),
			path: worktree.path.clone(),
			reused_existing: true,
		},
		retry_project_slug: issue
			.project_slug
			.clone()
			.expect("sample issue should carry a project slug"),
		dispatch_mode: IssueDispatchMode::Closeout,
		attempt_number: 1,
		run_id: String::from("run-closeout-retargeted-pr"),
		retry_budget_base: 0,
	};
	let error = orchestrator::cleanup_completed_post_review_lane(
		&config,
		&workflow,
		&state_store,
		&issue_run,
	)
	.expect_err("cleanup must fail closed when the merged PR target branch drifted");

	assert!(
		error.to_string().contains("expected PR") && error.to_string().contains("release/1.x"),
		"cleanup should surface the authoritative PR target-branch mismatch"
	);
	assert!(
		worktree.path.exists(),
		"cleanup must preserve the retained worktree when the merged PR target branch drifted"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping must remain so cleanup can retry after a corrected handoff"
	);
}

#[test]
fn cleanup_completed_post_review_lane_deletes_local_lane_branch_after_worktree_cleanup() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = sample_issue("Done", &[]);
	let _tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/181";
	let _path_guard = install_fake_merged_pr_gh_response(&temp_dir, &worktree, pr_url, &head_oid);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

	initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);

	assert!(
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);

	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let issue_run =
		sample_closeout_issue_run(&issue, &worktree, "run-closeout-local-branch-cleanup");

	orchestrator::cleanup_completed_post_review_lane(&config, &workflow, &state_store, &issue_run)
		.expect("cleanup should succeed once merged closeout is authoritative");

	assert!(!worktree.path.exists(), "cleanup should remove the retained worktree path");
	assert!(
		git_output(config.repo_root(), &["branch", "--list", &worktree.branch_name]).is_empty(),
		"cleanup should delete the retained local lane branch"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"cleanup should clear retained worktree state after local branch deletion succeeds"
	);
}

#[test]
fn cleanup_completed_post_review_lane_uses_persisted_handoff_marker() {
	let cases = [
		(
			"matching current branch",
			"https://github.com/hack-ink/decodex/pull/181",
			"run-closeout-cleanup",
			1,
			"run-closeout-cleanup",
		),
		(
			"stale run identity with current marker",
			"https://github.com/hack-ink/decodex/pull/182",
			"run-closeout-stale",
			7,
			"run-closeout-current",
		),
	];

	for (case_name, pr_url, marker_run_id, marker_attempt, issue_run_id) in cases {
		let (temp_dir, base_config, workflow) = temp_project_layout();
		let config = service_config_with_github_token_env_var(&base_config, "HOME");
		let issue = sample_issue("Done", &[]);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let worktree_manager =
			WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
		let worktree = worktree_manager
			.ensure_worktree(&issue.identifier, false)
			.expect("retained closeout worktree should exist");
		let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
		let _path_guard =
			install_fake_merged_pr_gh_response(&temp_dir, &worktree, pr_url, &head_oid);
		let remote_root =
			config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

		initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
		git_status_success(
			config.repo_root(),
			&["push", "origin", &format!("HEAD:{}", worktree.branch_name)],
		);
		seed_review_handoff_marker_value(
			&state_store,
			config.service_id(),
			&issue.id,
			&ReviewHandoffMarker::new(
				marker_run_id,
				marker_attempt,
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
			.expect("worktree mapping should record");

		let issue_run = sample_closeout_issue_run(&issue, &worktree, issue_run_id);

		orchestrator::cleanup_completed_post_review_lane(
			&config,
			&workflow,
			&state_store,
			&issue_run,
		)
		.expect("cleanup should use the persisted handoff marker");

		assert!(
			!worktree.path.exists(),
			"cleanup should remove the retained worktree path for {case_name}"
		);
	}
}

#[test]
fn cleanup_completed_post_review_lane_preserves_worktree_when_local_branch_delete_fails() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let issue = sample_issue("Done", &[]);
	let _tracker = FakeTracker::new(vec![issue.clone()]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/182";
	let _path_guard = install_fake_merged_pr_gh_response(&temp_dir, &worktree, pr_url, &head_oid);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");
	let blocking_worktree = config.worktree_root().join("blocking-local-branch-delete");

	initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);

	assert!(
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);
	assert!(
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(&worktree.path)
			.args(["checkout", "--quiet", "--detach"])
			.status()
			.expect("git checkout --detach should run")
			.success()
	);
	assert!(
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args([
				"worktree",
				"add",
				"--quiet",
				blocking_worktree.to_string_lossy().as_ref(),
				&worktree.branch_name,
			])
			.status()
			.expect("git worktree add should run")
			.success()
	);

	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree mapping should record");

	let issue_run =
		sample_closeout_issue_run(&issue, &worktree, "run-closeout-local-branch-delete-blocked");
	let error = orchestrator::cleanup_completed_post_review_lane(
		&config,
		&workflow,
		&state_store,
		&issue_run,
	)
	.expect_err("cleanup should fail closed when another worktree still holds the lane branch");

	assert!(
		error.to_string().contains("Failed to delete retained local branch"),
		"cleanup should surface the local branch deletion failure"
	);
	assert!(
		worktree.path.exists(),
		"cleanup must preserve the retained worktree when local branch deletion fails"
	);
	assert!(
		state_store
			.review_handoff_marker(config.service_id(), &issue.id, &worktree.branch_name)
			.expect("review handoff marker read should succeed")
			.is_some(),
		"cleanup must preserve the runtime review handoff marker so closeout can retry later"
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_some(),
		"worktree mapping must remain so cleanup can retry later"
	);
}
