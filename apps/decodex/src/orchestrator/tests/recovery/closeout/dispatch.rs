#[test]
fn closeout_dispatch_completes_merged_lane_without_agent_turn() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = issue_with_completed_state(sample_issue("In Review", &[active_label.as_str()]));
	let mut completed_issue = issue.clone();

	completed_issue.state =
		TrackerState { id: String::from("state-done"), name: String::from("Done") };

	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![
			vec![issue.clone()],
			vec![issue.clone()],
			vec![completed_issue.clone()],
			vec![completed_issue.clone()],
		],
	);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/701";
	let _path_guard = install_fake_closeout_gh_responses(&temp_dir, &worktree, pr_url, &head_oid);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

	initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	route_origin_github_url_to_local_bare_repo(config.repo_root(), &remote_root);

	assert!(
		crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(config.repo_root())
			.args(["push", "origin", &format!("HEAD:{}", worktree.branch_name)])
			.status()
			.expect("git push lane branch should run")
			.success()
	);

	seed_review_handoff_marker(
		&state_store,
		config.service_id(),
		&issue.id,
		&worktree.branch_name,
		pr_url,
		&head_oid,
	);

	let issue_run = sample_closeout_issue_run(&issue, &worktree, "pub-701-attempt-3-closeout");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "starting")
		.expect("run attempt should record");

	let summary =
		orchestrator::execute_issue_run(&tracker, &config, &workflow, &state_store, issue_run)
			.expect("deterministic closeout should complete");

	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		[(issue.id.clone(), String::from("state-done"))]
	);
	assert_eq!(tracker.comments.borrow().len(), 2);
	assert!(tracker.comments.borrow()[0].contains("decodex closeout completed"));

	let event_types = tracker
		.comments
		.borrow()
		.iter()
		.filter_map(|comment| records::parse_linear_execution_event_record(comment))
		.map(|record| record.event_type)
		.collect::<Vec<_>>();

	assert_eq!(event_types, vec![String::from("closeout"), String::from("cleanup_complete")]);
	assert!(!worktree.path.exists(), "deterministic closeout should remove the retained worktree");
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none(),
		"deterministic closeout should clear retained worktree state"
	);
	assert!(
		state_store.lease_for_issue(&issue.id).expect("lease lookup should succeed").is_none(),
		"deterministic closeout should not leave an run lease"
	);
	assert_eq!(
		tracker.label_removals.borrow().len(),
		2,
		"deterministic closeout should clear active and queue lane labels"
	);
}

#[test]
fn direct_closeout_dispatch_reuses_completed_handoff_run_identity_for_record_and_summary() {
	let fixture = closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);

	assert_closeout_lane_ready(&fixture);

	let summary = orchestrator::run_target_issue_once(TargetIssueRunContext {
		tracker: &fixture.tracker,
		project: &fixture.config,
		workflow: &fixture.workflow,
		state_store: &fixture.state_store,
		issue_id: &fixture.issue.id,
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: false,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
	.expect("direct retained closeout should run")
	.expect("closeout summary should be printed");
	let message = orchestrator::format_run_once_summary(&summary, false);

	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, fixture.completed_run_id);
	assert_eq!(summary.attempt_number, 1);
	assert!(
		message.contains(&format!("run_id={}", fixture.completed_run_id)),
		"terminal summary should print the completed handoff run id: {message}"
	);
	assert!(
		!message.contains("attempt-2"),
		"direct closeout must not look like a hidden retry: {message}"
	);

	let issue_comments = fixture.tracker.issue_comments.borrow();
	let closeout_comments =
		issue_comments.get(&fixture.issue.id).expect("closeout should write an issue comment");

	assert!(
		closeout_comments.iter().any(|comment| {
			records::parse_linear_execution_event_record(&comment.body).is_some_and(|record| {
				record.event_type == "closeout"
					&& record.run_id == fixture.completed_run_id
					&& record.attempt_number == 1
					&& record.branch.as_deref() == Some(fixture.worktree.branch_name.as_str())
					&& record.pr_url.as_deref() == Some(fixture.pr_url.as_str())
			})
		}),
		"direct closeout event should reuse the completed handoff identity"
	);
	assert!(
		fixture
			.state_store
			.run_attempt_for_issue_attempt(&fixture.issue.id, 2)
			.expect("second attempt lookup should succeed")
			.is_none(),
		"successful direct closeout should not create an invisible second attempt"
	);
}

#[test]
fn same_run_closeout_reuses_matching_active_handoff_lease() {
	let fixture = closeout_identity_fixture();
	let _keep_fixture_alive = (&fixture._temp_dir, &fixture._path_guard);

	assert_closeout_lane_ready(&fixture);

	fixture
		.state_store
		.upsert_lease(
			fixture.config.service_id(),
			&fixture.issue.id,
			&fixture.completed_run_id,
			"In Review",
		)
		.expect("handoff lease should recover before same-run closeout");

	let source_summary = RunSummary {
		project_id: fixture.config.service_id().to_owned(),
		issue_id: fixture.issue.id.clone(),
		issue_identifier: fixture.issue.identifier.clone(),
		issue_state: String::from("In Review"),
		initial_issue_state: String::from("Todo"),
		retry_project_slug: fixture.config.service_id().to_owned(),
		dispatch_mode: IssueDispatchMode::Normal,
		branch_name: fixture.worktree.branch_name.clone(),
		worktree_path: fixture.worktree.path.clone(),
		attempt_number: 1,
		run_id: fixture.completed_run_id.clone(),
		continuation_pending: false,
	};
	let summary = orchestrator::run_retained_closeout_for_handoff_summary(
		&fixture.tracker,
		&fixture.config,
		&fixture.workflow,
		&fixture.state_store,
		&source_summary,
	)
	.expect("same-run retained closeout should run")
	.expect("same-run retained closeout should produce a summary");

	assert_eq!(summary.dispatch_mode, IssueDispatchMode::Closeout);
	assert_eq!(summary.run_id, fixture.completed_run_id);
	assert!(
		fixture
			.state_store
			.lease_for_issue(&fixture.issue.id)
			.expect("lease lookup should succeed")
			.is_none(),
		"same-run closeout should clear the recovered handoff lease"
	);
}

#[test]
fn closeout_completed_state_check_skips_redundant_transition() {
	let (_temp_dir, _config, workflow) = temp_project_layout();
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = issue_with_completed_state(sample_issue("Done", &[active_label.as_str()]));
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let worktree = WorktreeSpec {
		branch_name: String::from("x/pubfi-pub-101"),
		issue_identifier: issue.identifier.clone(),
		path: PathBuf::from(".worktrees/PUB-101"),
		reused_existing: true,
	};
	let issue_run = sample_closeout_issue_run(&issue, &worktree, "pub-101-closeout-done");

	orchestrator::ensure_closeout_issue_completed_state(&tracker, &workflow, &issue_run)
		.expect("completed issue should not require another transition");

	assert!(
		tracker.state_updates.borrow().is_empty(),
		"already completed issues should not be transitioned again"
	);
}

#[test]
fn closeout_dispatch_validates_pr_before_marking_issue_done() {
	let (temp_dir, base_config, workflow) = temp_project_layout();
	let config = service_config_with_github_token_env_var(&base_config, "HOME");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = issue_with_completed_state(sample_issue("In Review", &[active_label.as_str()]));
	let tracker =
		FakeTracker::with_refresh_snapshots(vec![issue.clone()], vec![vec![issue.clone()]]);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&issue.identifier, false)
		.expect("retained closeout worktree should exist");
	let head_oid = git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/702";
	let _path_guard = install_fake_closeout_gh_responses_with_state(
		&temp_dir, &worktree, pr_url, &head_oid, "OPEN",
	);
	let remote_root =
		config.repo_root().parent().expect("repo root should have a parent").join("origin.git");

	initialize_closeout_cleanup_origin(config.repo_root(), &remote_root);
	route_origin_github_url_to_local_bare_repo(config.repo_root(), &remote_root);
	seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let issue_run = sample_closeout_issue_run(&issue, &worktree, "pub-702-attempt-1-closeout");

	state_store
		.record_run_attempt(&issue_run.run_id, &issue.id, issue_run.attempt_number, "starting")
		.expect("run attempt should record");

	let error =
		orchestrator::execute_issue_run(&tracker, &config, &workflow, &state_store, issue_run)
			.expect_err("unmerged PR should stop deterministic closeout");

	assert!(
		error.to_string().contains("must be merged before closeout completes"),
		"closeout should fail at PR validation: {error:?}"
	);
	assert!(
		tracker.state_updates.borrow().is_empty(),
		"closeout must not mark the issue done before PR validation succeeds"
	);
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex closeout completed")),
		"closeout must not write a closeout completion record when PR validation fails"
	);
	assert!(
		!tracker
			.comments
			.borrow()
			.iter()
			.any(|comment| comment.contains("decodex run failed and needs attention")),
		"closeout PR visibility races should remain retryable instead of terminal"
	);
	assert!(worktree.path.exists(), "failed closeout should preserve the retained worktree");
}
